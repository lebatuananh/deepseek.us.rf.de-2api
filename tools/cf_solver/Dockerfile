FROM python:3.11-slim-bookworm

# camoufox (Firefox) 运行所需的系统库
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates xvfb fonts-liberation \
        libgtk-3-0 libdbus-glib-1-2 libxt6 libx11-xcb1 libasound2 \
        libxcomposite1 libxdamage1 libxrandr2 libxfixes3 libxtst6 libpci3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt \
    && python -m camoufox fetch

COPY . .
EXPOSE 8001
CMD ["python", "boterdrop_wrapper.py"]
