//! 控制台 Web UI（P3-1）：模型列表、Key 状态、实时用量、solver 健康。
//!
//! 单文件自包含（内联 HTML/CSS/JS，无外部依赖、无 CDN），深色编辑台风格。
//! 访问需 `admin_enabled=true` 且通过 `X-Admin-Token` 或 `?token=` 校验。

use crate::api::{AppState, SharedState};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::Json;
use std::collections::HashMap;

/// 校验控制台访问权限（失败返回 Box<Response> 以缩小 Result 尺寸）。
fn check_admin(
    state: &AppState,
    headers: &HeaderMap,
    query_token: Option<&str>,
) -> Result<(), Box<Response>> {
    if !state.cfg.admin_enabled {
        return Err(Box::new(
            (
                StatusCode::NOT_FOUND,
                "Bảng điều khiển chưa được bật (đặt admin_enabled=true)",
            )
                .into_response(),
        ));
    }
    if state.cfg.admin_token.is_empty() {
        return Err(Box::new(
            (
                StatusCode::FORBIDDEN,
                "Bảng điều khiển đã bật nhưng chưa đặt admin_token, từ chối truy cập",
            )
                .into_response(),
        ));
    }
    let provided = headers
        .get("x-admin-token")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| query_token.map(|s| s.to_string()));
    match provided {
        Some(t) if verify_admin_token(&state.cfg.admin_token, &t) => Ok(()),
        _ => Err(Box::new(
            (
                StatusCode::UNAUTHORIZED,
                "Token bảng điều khiển không hợp lệ",
            )
                .into_response(),
        )),
    }
}

/// §6.6：校验控制台令牌。
///
/// 配置支持两种形式：
/// - 明文（向后兼容）
/// - `sha256:<hex>`（推荐的哈希存储：配置文件泄露 ≠ 令牌泄露）
///
/// 比较恒为**常量时间**。
fn verify_admin_token(configured: &str, provided: &str) -> bool {
    if let Some(hex) = configured.strip_prefix("sha256:") {
        let digest = sha256_hex(provided.as_bytes());
        // 两侧都规范化为小写后常量时间比较——运维常写大写 hex，
        // 直接比较会静默拒绝，属健壮性问题。
        let want = hex.trim().to_ascii_lowercase();
        constant_eq(&digest, &want)
    } else {
        constant_eq(provided, configured)
    }
}

/// SHA-256 → 小写 hex。
pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(data);
    let out = h.finalize();
    let mut s = String::with_capacity(out.len() * 2);
    for b in out {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn constant_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut d = 0u8;
    for i in 0..a.len() {
        d |= a[i] ^ b[i];
    }
    d == 0
}

/// `GET /admin` — 控制台页面。
pub async fn admin_page(
    State(state): State<SharedState>,
    Query(q): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    if let Err(r) = check_admin(&state, &headers, q.get("token").map(|s| s.as_str())) {
        return *r;
    }
    Html(ADMIN_HTML).into_response()
}

/// `GET /admin/api/status` — 控制台数据（JSON）。
pub async fn admin_status(
    State(state): State<SharedState>,
    Query(q): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    if let Err(r) = check_admin(&state, &headers, q.get("token").map(|s| s.as_str())) {
        return *r;
    }

    let models: Vec<serde_json::Value> = crate::models::catalog()
        .into_iter()
        .map(|m| {
            serde_json::json!({
                "id": m.id, "label": m.label, "provider": m.provider,
                "context_window": m.context_window, "default": m.default, "routable": m.routable,
            })
        })
        .collect();

    let stats = match state.ledger.stats(None).await {
        Ok(s) => serde_json::to_value(s).unwrap_or_default(),
        Err(e) => serde_json::json!({ "error": e.to_string() }),
    };

    let recent = state.ledger.recent(50).await.unwrap_or_default();

    Json(serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "models": models,
        "sessions": state.sessions.len(),
        "solver": state.upstream.solver_health(),
        "upstream": {
            "base_url": state.cfg.upstream_base_url,
            "bot_id": state.cfg.bot_id,
            "authed": state.upstream.is_authed_cached(),
        },
        // 6.4：运行时内部状态（可观测性）
        "runtime": {
            "cache_entries": state.cache.len(),
            "replay_responses": state.replay.len(),
            "breaker": state.upstream.breaker_state(),
        },
        "config": {
            "api_keys": state.cfg.api_keys.len(),
            "cache_ttl_secs": state.cfg.cache_ttl_secs,
            "ledger_enabled": state.ledger.enabled(),
            "ledger_retention_days": state.cfg.ledger_retention_days,
            "solvers": state.upstream.solver_count(),
            "rate_limit_per_sec": state.cfg.rate_limit_per_sec,
            "max_concurrency": state.cfg.max_concurrency,
            "max_request_bytes": state.cfg.max_request_bytes,
        },
        "stats": stats,
        "recent": recent,
    }))
    .into_response()
}

/// 控制台 HTML（自包含、无外部资源）。
const ADMIN_HTML: &str = r#"<!DOCTYPE html>
<html lang="vi"><head><meta charset="utf-8"/>
<meta name="viewport" content="width=device-width,initial-scale=1"/>
<title>deepseek-es-2api - Bảng điều khiển</title>
<style>
:root{--bg:#0d1117;--panel:#161b22;--line:#30363d;--fg:#e6edf3;--muted:#8b949e;
--accent:#3fb950;--accent2:#58a6ff;--warn:#d29922;--err:#f85149;--mono:ui-monospace,SFMono-Regular,Menlo,monospace}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--fg);font:14px/1.5 -apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif}
header{padding:18px 24px;border-bottom:1px solid var(--line);display:flex;align-items:baseline;gap:16px}
h1{font-size:17px;margin:0;letter-spacing:.5px}
.ver{color:var(--muted);font-family:var(--mono);font-size:12px}
main{padding:24px;display:grid;gap:20px;max-width:1200px}
.row{display:grid;grid-template-columns:repeat(auto-fit,minmax(180px,1fr));gap:14px}
.card{background:var(--panel);border:1px solid var(--line);border-radius:10px;padding:16px}
.card h2{font-size:12px;text-transform:uppercase;letter-spacing:.8px;color:var(--muted);margin:0 0 12px}
.metric{font-size:26px;font-weight:600;font-variant-numeric:tabular-nums}
.metric small{font-size:13px;color:var(--muted);font-weight:400}
table{width:100%;border-collapse:collapse;font-size:13px}
th,td{text-align:left;padding:8px 10px;border-bottom:1px solid var(--line)}
th{color:var(--muted);font-weight:500;font-size:11px;text-transform:uppercase;letter-spacing:.6px}
td.mono{font-family:var(--mono)}
.dot{display:inline-block;width:8px;height:8px;border-radius:50%;margin-right:6px}
.ok{background:var(--accent)}.bad{background:var(--err)}.warn{background:var(--warn)}
.badge{display:inline-block;padding:2px 7px;border-radius:999px;font-size:11px;border:1px solid var(--line);color:var(--muted)}
.badge.on{border-color:var(--accent);color:var(--accent)}
.empty{color:var(--muted);padding:12px 0;font-style:italic}
footer{padding:16px 24px;color:var(--muted);font-size:12px;border-top:1px solid var(--line)}
button{background:var(--panel);color:var(--fg);border:1px solid var(--line);border-radius:6px;padding:6px 12px;cursor:pointer;font-size:13px}
button:hover{border-color:var(--accent2);color:var(--accent2)}
</style></head><body>
<header><h1>deepseek-es-2api - Bảng điều khiển</h1><span class="ver" id="ver"></span>
<span style="flex:1"></span><button onclick="load()">Làm mới</button></header>
<main>
<div class="row" id="metrics"></div>
<div class="card"><h2>Mô hình</h2><div id="models"></div></div>
<div class="card"><h2>Tình trạng Solver</h2><div id="solver"></div></div>
<div class="card"><h2>Yêu cầu gần đây</h2><div id="recent"></div></div>
</main>
<footer>Token được truyền qua <code>?token=</code> hoặc <code>X-Admin-Token</code>. Dữ liệu được đọc theo thời gian thực, không lưu cache.</footer>
<script>
const qs=new URLSearchParams(location.search);const token=qs.get('token')||'';
async function api(p){const r=await fetch(p+(token?('?token='+encodeURIComponent(token)):''),
{headers:token?{'X-Admin-Token':token}:{}});if(!r.ok)throw new Error('HTTP '+r.status);return r.json();}
function el(t,c){const e=document.createElement(t);if(c)e.className=c;return e;}
function metric(title,value,sub){const d=el('div','card');d.innerHTML=`<h2>${title}</h2><div class="metric">${value}</div>`+
(sub?`<div style="color:var(--muted);font-size:12px;margin-top:4px">${sub}</div>`:'');return d;}
function table(cols,rows){
  if(!rows.length)return '<div class="empty">Chưa có dữ liệu</div>';
  let h='<table><thead><tr>'+cols.map(c=>`<th>${c}</th>`).join('')+'</tr></thead><tbody>';
  h+=rows.join('');return h+'</tbody></table>';
}
async function load(){
  try{
    const d=await api('/admin/api/status');
    document.getElementById('ver').textContent='v'+d.version;
    const s=d.stats||{};const m=document.getElementById('metrics');m.innerHTML='';
    m.append(metric('Tổng yêu cầu',(s.total_requests??0).toLocaleString()));
    m.append(metric('Prompt tokens',(s.total_prompt_tokens??0).toLocaleString()));
    m.append(metric('Completion tokens',(s.total_completion_tokens??0).toLocaleString()));
    m.append(metric('Cache trúng',(s.cache_hits??0).toLocaleString()));
    m.append(metric('Lỗi',(s.errors??0).toLocaleString()));
    m.append(metric('Độ trễ trung bình',(s.avg_latency_ms??0)+' <small>ms</small>'));
    m.append(metric('Phiên hoạt động',(d.sessions??0).toLocaleString()));
    m.append(metric('Key phía client',(d.config?.api_keys??0).toString(),'Số key trong danh sách cho phép'));
    document.getElementById('models').innerHTML=table(['ID','Nhãn','Nhà cung cấp','Ngữ cảnh','Trạng thái'],
      (d.models||[]).map(x=>`<tr><td class="mono">${x.id}</td><td>${x.label}</td><td>${x.provider}</td>`+
        `<td class="mono">${x.context_window.toLocaleString()}</td>`+
        `<td>${x.default?'<span class="badge on">Mặc định</span>':''} ${x.routable?'':'<span class="badge">Bí danh</span>'}</td></tr>`));
    document.getElementById('solver').innerHTML=table(['Địa chỉ','Trạng thái','Lỗi liên tiếp','Thành công gần nhất','Thành công/Thất bại'],
      (d.solver||[]).map(x=>`<tr><td class="mono">${x.url}</td>`+
        `<td><span class="dot ${x.healthy?'ok':'bad'}"></span>${x.healthy?'Khỏe mạnh':'Bất thường'}</td>`+
        `<td>${x.consecutive_failures}</td><td>${x.last_success_secs!=null?x.last_success_secs.toFixed(1)+'s':'—'}</td>`+
        `<td class="mono">${x.total_success}/${x.total_failure}</td></tr>`));
    document.getElementById('recent').innerHTML=table(['Thời gian','Mô hình','Key','Tok(vào/ra)','Độ trễ','Trạng thái'],
      (d.recent||[]).map(x=>{const t=new Date(x.ts*1000).toLocaleTimeString();
        return `<tr><td class="mono">${t}</td><td class="mono">${x.model}</td><td class="mono">${x.key_id}</td>`+
        `<td class="mono">${x.prompt_tokens}/${x.completion_tokens}</td>`+
        `<td class="mono">${x.latency_ms}ms</td>`+
        `<td><span class="dot ${x.status<400?'ok':'bad'}"></span>${x.status}${x.cached?' <span class="badge">Cache</span>':''}</td></tr>`;}));
  }catch(e){
    document.getElementById('metrics').innerHTML=`<div class="card" style="color:var(--err)">Tải thất bại: ${e.message} (kiểm tra ?token= có đúng không)</div>`;
  }
}
load();setInterval(load,5000);
</script></body></html>"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_eq_works() {
        assert!(constant_eq("abc", "abc"));
        assert!(!constant_eq("abc", "abd"));
        assert!(!constant_eq("abc", "ab"));
    }

    #[test]
    fn html_is_self_contained() {
        // 反模板设计 + 无外部资源
        assert!(!ADMIN_HTML.contains("http://"));
        assert!(!ADMIN_HTML.contains("https://cdn"));
        assert!(!ADMIN_HTML.contains("<script src"));
        assert!(!ADMIN_HTML.contains("<link rel=\"stylesheet\""));
        assert!(ADMIN_HTML.contains("admin/api/status"));
        assert!(ADMIN_HTML.contains("--bg:#0d1117"), "应有明确设计令牌");
    }

    // ── §6.6：admin_token 哈希存储校验 ──────────────────────

    #[test]
    fn sha256_hex_known_vector() {
        // 空串的 SHA-256（已知向量）
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        // "abc" 的 SHA-256（已知向量）
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn verify_token_plaintext_backcompat() {
        // 明文配置（向后兼容）
        assert!(verify_admin_token("s3cret", "s3cret"));
        assert!(!verify_admin_token("s3cret", "wrong"));
    }

    #[test]
    fn verify_token_sha256_prefix() {
        let hex = sha256_hex(b"s3cret");
        let configured = format!("sha256:{hex}");
        assert!(verify_admin_token(&configured, "s3cret"));
        assert!(!verify_admin_token(&configured, "wrong"));
        // 明文假配置（前缀但值不是正确哈希）必须拒绝
        assert!(!verify_admin_token("sha256:deadbeef", "s3cret"));
    }

    #[test]
    fn verify_token_sha256_is_case_insensitive_on_config() {
        // 配置里的 hex 允许大写（trim 后比较）；注意 hash 输出恒小写
        let hex = sha256_hex(b"tok").to_ascii_uppercase();
        let configured = format!("sha256:  {hex}  "); // 含多余空格
        assert!(
            verify_admin_token(&configured, "tok"),
            "配置侧 hex 大小写/空白应被容忍"
        );
    }
}
