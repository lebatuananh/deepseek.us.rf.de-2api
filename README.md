# deepseek-es-2api

API gateway chuyển [deepseek.es](https://deepseek.es) (chat WordPress + AIPKit) thành API **tương thích OpenAI** (`/v1/chat/completions`)
và **tương thích Anthropic** (`/v1/messages`), **kết nối với cf_solver bên ngoài để giải Cloudflare Turnstile**.

> Upstream dùng giao thức SSE không chuẩn. Gateway này xử lý xác thực (Turnstile), quản lý phiên và dịch giao thức ở phía Rust.
> **Đã được kiểm chứng E2E thực tế**: giải Turnstile → cookie bảo mật → luồng SSE → đầu ra tương thích OpenAI/Anthropic.

---

## Kiến trúc

```
┌─────────────┐   OpenAI/Anthropic    ┌──────────────────────────┐
│  Client     │ ───────────────────▶ │  deepseek-es-2api (Rust)  │
│ (Cursor/    │                       │  · Dịch giao thức         │
│  Claude Code)│ ◀─────────────────── │    (SSE↔OpenAI)           │
└─────────────┘     SSE / JSON        │  · Ánh xạ phiên (conv_uuid)│
                                      │  · Cookie pool + tự phục hồi│
                                      └───────────┬──────────────┘
                                                  │
                          ┌───────────────────────┼───────────────────────┐
                          ▼                       ▼                       ▼
                  ┌───────────────┐      ┌────────────────┐     ┌─────────────────┐
                  │ deepseek.es   │      │ cf_solver      │     │ Turnstile       │
                  │ admin-ajax    │      │ (camoufox)     │     │ (giải)          │
                  │ + SSE         │      │ 127.0.0.1:8001 │     │                 │
                  └───────────────┘      └────────────────┘     └─────────────────┘
```

**Luồng xác thực** (tự động kích hoạt lần đầu hoặc khi hết hiệu lực):
```
solve Turnstile (cf_solver) → POST deepseek_ts_verify → dsts_ok cookie
  → POST aipkit_get_frontend_chat_nonce → nonce
  → POST aipkit_cache_sse_message → cache_key
  → GET  aipkit_frontend_chat_stream (SSE) → message_start/delta/done
```

---

## Bắt đầu nhanh

### 1. Khởi động cf_solver (giải Cloudflare Turnstile)

Cần Python 3.11 + camoufox (xem `tools/cf_solver/requirements.txt`):

```bash
cd tools/cf_solver
pip install -r requirements.txt
python -m camoufox fetch          # tải trình duyệt lần đầu
python boterdrop_wrapper.py       # lắng nghe 0.0.0.0:8001, đợi "Pool siap"
```

### 2. Biên dịch và khởi động gateway

```bash
cargo build --release
cp config.example.json config.json   # chỉnh sửa tùy nhu cầu
./target/release/deepseek-es-2api --config config.json
```

Mặc định lắng nghe tại `http://127.0.0.1:47833`.

### 3. Kết nối client

**OpenAI SDK / Cursor / Continue**
```
Base URL: http://127.0.0.1:47833/v1
API Key:  sk-local (điền tùy ý khi chưa cấu hình api_keys)
Model:    deepseek-es
```

**Claude Code (giao thức Anthropic)**
```bash
export ANTHROPIC_BASE_URL=http://127.0.0.1:47833
export ANTHROPIC_API_KEY=sk-local
claude
```

---

## Endpoint API

| Endpoint | Method | Mô tả |
|----------|--------|-------|
| `/v1/chat/completions` | POST | Chat OpenAI (streaming / không streaming) |
| `/v1/messages` | POST | Chat Anthropic (streaming / không streaming), dành cho Claude Code |
| `/v1/messages/count_tokens` | POST | Ước tính token |
| `/v1/models` | GET | Danh sách mô hình |
| `/healthz` | GET | Kiểm tra sức khỏe |
| `/admin` | GET | Web UI bảng điều khiển (cần `admin_enabled` + `admin_token`) |
| `/admin/api/status` | GET | Dữ liệu bảng điều khiển (JSON) |
| `/v1/responses/{id}` | GET | **Phát lại khi mất kết nối**: phát lại các sự kiện chưa nhận được của một phản hồi streaming |
| `/v1/balance` | GET | Hạn mức/số dư upstream (của **danh tính gateway**, không phải số dư người dùng) |
| `/v1/conversations` | GET / DELETE | Liệt kê / xóa phiên upstream (`?id=`; header `x-session-id` để phân biệt phiên) |

### Bảng điều khiển

Sau khi đặt `admin_enabled=true` và `admin_token`, truy cập `http://127.0.0.1:47833/admin?token=<token>`
(hoặc dùng header `X-Admin-Token`). Hiển thị: danh sách mô hình, thống kê sử dụng thời gian thực, tình trạng solver, yêu cầu gần đây, tổng quan cấu hình.
Là trang đơn tự chứa (không dùng CDN bên ngoài), khuyến nghị chỉ bật trên máy cục bộ.

### Tính năng nâng cao

| Tính năng | Cấu hình | Mô tả |
|-----------|----------|-------|
| **Cache phản hồi theo yêu cầu** | `cache_ttl_secs` | Dùng lại trong thời gian ngắn cho cùng một prompt (**không có ngữ nghĩa phiên**); có hiệu lực ở **cả OpenAI và Anthropic**; `temperature > 0` thì không cache |
| **Sổ cái sử dụng** | `ledger_path` | Lưu bền bằng SQLite (`usage.db`); để trống = tắt |
| **Cân bằng tải nhiều solver** | `solver_urls` | Round-robin + dò sức khỏe + chuyển dự phòng; có thể trộn trình duyệt và backend API bên thứ ba |
| **Solver API bên thứ ba** | `tools/api_solver` | Adapter HTTP thuần (capsolver/2captcha), tiếp quản khi camoufox ngừng hoạt động |
| **Cookie cài sẵn** | `initial_cookies` | Dùng trình duyệt của bạn vượt CF một lần, thu thập bằng `scripts/collect-cookie.mjs`, **không phụ thuộc solver** |
| **Chèn ngôn ngữ** | `system_prompt_suffix` | Thêm chỉ dẫn system (ví dụ "trả lời bằng ngôn ngữ của người dùng") |
| **Hội thoại nhiều lượt** | — | Toàn bộ lịch sử tin nhắn do client gửi được render vào prompt (xem bên dưới) |
| **Gọi công cụ** | — | `tools`/`tool_calls` ở cấp giao thức (OpenAI) và `tools`/`tool_use` (Anthropic), xem bên dưới |
| **Gọi công cụ giả** | `pseudo_tools_enabled` | Gateway **thực thi công cụ cục bộ** (khác với "Gọi công cụ" ở trên, xem bên dưới) |
| **Usage khi streaming** | — | `stream_options.include_usage` → trả về frame usage trước `[DONE]` |
| **Phát lại khi mất kết nối** | — | Sự kiện SSE có `id:`; xem phần giới hạn bên dưới |

> **Gọi công cụ (cấp giao thức, v0.7.0)**: bật khi client truyền `tools` trong yêu cầu. Dù upstream **không hỗ trợ
> function calling gốc**, gateway thực hiện qua "chèn mô tả công cụ → phân tích khối ` ```tool ` trong đầu ra của mô hình → tạo cấu trúc giao thức chuẩn":
> OpenAI trả về `choices[].message.tool_calls` + `finish_reason:"tool_calls"`;
> Anthropic trả về `content[].type=="tool_use"` + `stop_reason:"tool_use"`.
> Kết quả client gửi lại (OpenAI `role:"tool"` / Anthropic `tool_result`) sẽ được chuyển thành prompt để mô hình trả lời tiếp.
> Hỗ trợ streaming: `delta.tool_calls` (OpenAI) / `input_json_delta` (Anthropic); nội dung khối công cụ không bị rò vào luồng văn bản.

> **Gọi công cụ giả (do gateway chèn, khác với trên)**: khi `pseudo_tools_enabled=true`, gateway chèn mô tả công cụ tích hợp
> (`get_time`/`echo`) và **thực thi cục bộ trên gateway**, kết quả được điền lại dưới dạng văn bản, dùng cho trường hợp client không có khả năng thực thi công cụ.
> Hai chế độ loại trừ nhau: nếu yêu cầu có `tools` thì dùng công cụ cấp giao thức, ngược lại dựa vào `pseudo_tools_enabled`.

> **Ngữ nghĩa hội thoại nhiều lượt**: dù upstream tiếp nối lịch sử theo `conversation_uuid`, các client OpenAI/Anthropic chuẩn
> **gửi toàn bộ lịch sử mỗi lượt**, và đa số SDK mặc định không đặt trường `user`: khi đó gateway không thể ánh xạ phiên ổn định,
> nếu chỉ lấy tin nhắn cuối sẽ **mất lịch sử**. Vì vậy gateway **render toàn bộ lịch sử vào prompt theo vai trò khi nhiều lượt**
> (`user: ... / assistant: ...`), giúp gateway không giữ trạng thái và mỗi lượt có thể tái hiện độc lập;
> yêu cầu một lượt giữ nguyên (không thêm tiền tố vai trò).

> **Toàn vẹn UTF-8**: SSE của upstream có thể cắt ký tự đa byte (CJK/emoji) ngay ở ranh giới chunk mạng.
> Gateway phân tích bằng **bộ đệm mức byte** và chỉ giải mã tại ranh giới sự kiện hoàn chỉnh, đảm bảo ký tự đa byte không bị hỏng thành `�`.

> **Giới hạn khi mất kết nối (công khai trung thực)**: SSE của upstream **không gửi dòng `id:`** và `cache_key` chỉ dùng một lần,
> nên **không thể** thực hiện việc tiếp tục luồng upstream bằng `Last-Event-ID` đúng nghĩa. Gateway triển khai **phát lại từ bộ đệm**:
> phản hồi streaming có header `x-response-id`; sau khi client mất kết nối có thể `GET /v1/responses/<id>` kèm
> `Last-Event-ID` (hoặc `?after=N`) để **lấy lại các sự kiện chưa nhận trong cùng tiến trình**;
> nếu tiến trình khởi động lại hoặc bộ đệm hết hạn (TTL mặc định 300s) sẽ trả 409, client cần gửi lại yêu cầu.

---

## Cấu hình (`config.json`)

| Trường | Mặc định | Mô tả |
|--------|----------|-------|
| `listen_addr` | `127.0.0.1:47833` | Địa chỉ lắng nghe |
| `upstream_base_url` | `https://deepseek.es` | Upstream |
| `bot_id` | `27623` | ID bot AIPKit |
| `sitekey` | `0x4AAAAAADlLZ3ljqZP6cQwq` | Sitekey Turnstile |
| `cf_solver_url` | `http://127.0.0.1:8001` | Địa chỉ solver |
| `api_keys` | `[]` | Key phía client; rỗng = chỉ cho phép máy cục bộ. Hỗ trợ `"sk-x"` hoặc `{"key":"sk-x","expires_at":<unix giây>}` (hết hạn tự động từ chối) |
| `proxy` | `null` | Proxy đầu ra tới upstream (ví dụ `http://127.0.0.1:10808`) |
| `initial_cookies` | `""` | Cookie cài sẵn (`"dsts_ok=1; dsts=<hash>"`): **bỏ qua hoàn toàn solver**, thu thập bằng `scripts/collect-cookie.mjs` |
| `user_agent` | `Chrome/150` | UA trình duyệt (**phải trùng với cf_solver**, nếu không cookie sẽ không gắn được) |
| `default_model` | `deepseek-es` | ID mô hình mặc định (có thể tùy chỉnh, `resolve_model` dùng làm giá trị dự phòng) |
| `solver_timeout_secs` | `120` | Timeout giải Turnstile (giây) |
| `http_timeout_secs` | `120` | Timeout đọc nhàn rỗi của yêu cầu upstream (giây; SSE không có timeout tổng, tính giữa hai lần đọc) |
| `cookie_ttl_secs` | `1800` | TTL cache cookie bảo mật (còn 20% thì prefetch gia hạn nền) |
| `connect_timeout_secs` | `20` | Timeout thiết lập kết nối upstream (luồng SSE không có timeout tổng, chỉ có kết nối + đọc nhàn rỗi) |
| `cors_allow_origins` | `[]` | CORS; rỗng = tắt |
| `solver_retries` | `2` | Số lần thử lại khi giải thất bại (exponential backoff) |
| `breaker_fail_threshold` | `5` | Ngắt mạch xác thực: ngưỡng thất bại liên tiếp |
| `breaker_cooldown_secs` | `30` | Ngắt mạch xác thực: số giây làm nguội |
| `max_concurrency` | `0` | Số đồng thời tối đa **toàn cục** (0 = không giới hạn). Lưu ý: chỉ có hiệu lực trước khi handler trả header phản hồi, không giới hạn thời lượng luồng SSE đã thiết lập |
| `rate_limit_per_sec` | `0` | Giới hạn tốc độ endpoint mỗi giây (0 = không giới hạn, cửa sổ cố định, dùng chung toàn cục; đặt trước bước xác thực nên yêu cầu chưa xác thực cũng được tính) |
| `admin_enabled` | `false` | Bật bảng điều khiển |
| `admin_token` | `""` | Token bảng điều khiển (bắt buộc khi bật); hỗ trợ dạng băm `sha256:<hex>` (lộ cấu hình ≠ lộ token) |
| `admin_rate_limit_per_sec` | `10` | Giới hạn tốc độ riêng cho bảng điều khiển (chống dò token) |
| `cache_ttl_secs` | `300` | TTL cache phản hồi (0 = tắt) |
| `cache_max_entries` | `1000` | Số mục tối đa của cache phản hồi |
| `cache_min_chars` | `0` | Kết quả ngắn hơn độ dài này sẽ không được cache |
| `ledger_path` | `usage.db` | Đường dẫn sổ cái SQLite (rỗng = tắt) |
| `ledger_retention_days` | `30` | Số ngày giữ sổ cái (0 = không dọn); bản ghi quá hạn tự động xóa mỗi 6 giờ |
| `solver_urls` | `[]` | Địa chỉ nhiều solver (round-robin + chuyển dự phòng). Có thể trộn camoufox (tools/cf_solver) và adapter API bên thứ ba (tools/api_solver) |
| `system_prompt_suffix` | `""` | Chỉ dẫn system được thêm vào (ngôn ngữ/phong cách) |
| `pseudo_tools_enabled` | `false` | Bật công cụ giả (chèn mô tả công cụ; mô hình xuất khối ` ```tool ` → thực thi cục bộ) |
| `max_response_bytes` | `2097152` | Giới hạn tổng hợp khi không streaming (chống phản hồi quá lớn) |
| `max_request_bytes` | `8388608` | Giới hạn thân yêu cầu (mặc định 8MB; client ngữ cảnh dài có thể tăng) |

Có thể ghi đè bằng biến môi trường: `LISTEN_ADDR` `UPSTREAM_BASE_URL` `BOT_ID` `SITEKEY` `CF_SOLVER_URL` `API_KEYS` `PROXY` `DEFAULT_MODEL` `CORS_ALLOW_ORIGINS` `HTTP_TIMEOUT_SECS` `CONNECT_TIMEOUT_SECS` `USER_AGENT`

> **Bảo mật**: khi địa chỉ lắng nghe **không phải loopback** (như `0.0.0.0`) mà `api_keys` rỗng, gateway **từ chối khởi động** (fail-fast). Nếu thực sự cần mở ra Internet không xác thực, phải đặt rõ `ALLOW_INSECURE_PUBLIC=1` (nguy hiểm).

---

## Mô hình

Frontend upstream không để lộ ID mô hình thật (do cấu hình bot phía server quyết định). Qua thực nghiệm, trang này chỉ có **một bot duy nhất** (`bot_id=27623`, `provider=DeepSeek`), proxy **không thể chuyển provider**.

Do đó gateway này **chỉ cung cấp một mô hình thật**:

| ID mô hình | Mô tả |
|------------|-------|
| `deepseek-es` | Mô hình thật duy nhất (`routable: true`) |

> Các bí danh cũ (`deepseek-es-openai` / `-claude` / `-google` / `-openrouter` / `-xai`) vẫn được chấp nhận để tương thích client cũ, nhưng **không còn là mô hình độc lập**: chúng được phân giải về `deepseek-es` và không còn xuất hiện trong `/v1/models`. Mục trong `/v1/models` có các trường `routable` và `alias_of` để ghi nhận trung thực.

---

## Phát triển và kiểm thử

```bash
# Unit test + integration test (mock upstream, 211 mục)
cargo test

# Định dạng và kiểm tra tĩnh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings

# E2E thực tế (cần cf_solver đang chạy)
node scripts/e2e-gateway.mjs "Di hola en una frase"

# Kiểm thử tải
node scripts/loadtest.mjs 4 12
```

**Kết quả đo thực tế** (upstream thật):
| Chỉ số | Giá trị |
|--------|---------|
| Unit + integration | 211/211 đạt |
| E2E OpenAI streaming | ✅ Phản hồi thật |
| E2E Anthropic streaming | ✅ Chuỗi sự kiện đầy đủ |
| Kiểm thử tải 8 đồng thời × 24 yêu cầu | 100% thành công, p50=1.86s, QPS 3.62 |

---

## Tài liệu phân tích chuyên sâu

Thư mục `分析文档/` chứa phân tích reverse engineering đầy đủ (00-09), gồm giao thức upstream, sự kiện SSE, cấu trúc cấu hình, cơ chế hạn mức, báo cáo xác thực E2E.

---

## Tuyên bố miễn trừ trách nhiệm

Dự án này chỉ dùng cho **học tập và nghiên cứu**. Vui lòng tuân thủ điều khoản dịch vụ của upstream, không được lạm dụng hoặc vượt hạn mức để trục lợi.
