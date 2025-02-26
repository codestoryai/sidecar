# Sidecar Deployment Guide

This guide explains how to deploy Sidecar in various environments.

## Deployment Options

### 1. Docker Deployment

```dockerfile
# Dockerfile
FROM rust:1.73 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM node:18 as frontend-builder
WORKDIR /app
COPY frontend .
RUN npm install && npm run build

FROM debian:bullseye-slim
COPY --from=builder /app/target/release/sidecar /usr/local/bin/
COPY --from=frontend-builder /app/dist /usr/share/sidecar/frontend
EXPOSE 3001
CMD ["sidecar"]
```

Deploy with Docker Compose:
```yaml
# docker-compose.yml
version: '3.8'
services:
  sidecar:
    build: .
    ports:
      - "3001:3001"
    environment:
      - OPENAI_API_KEY=${OPENAI_API_KEY}
      - ANTHROPIC_API_KEY=${ANTHROPIC_API_KEY}
    volumes:
      - ./data:/data
```

### 2. Kubernetes Deployment

```yaml
# kubernetes/deployment.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: sidecar
spec:
  replicas: 3
  selector:
    matchLabels:
      app: sidecar
  template:
    metadata:
      labels:
        app: sidecar
    spec:
      containers:
      - name: sidecar
        image: sidecar:latest
        ports:
        - containerPort: 3001
        env:
        - name: OPENAI_API_KEY
          valueFrom:
            secretKeyRef:
              name: llm-secrets
              key: openai-key
```

### 3. Bare Metal Deployment

1. **System Requirements:**
   - 4+ CPU cores
   - 8GB+ RAM
   - 20GB+ storage
   - Linux/Unix OS

2. **Installation:**
```bash
# Install dependencies
apt-get update && apt-get install -y \
  build-essential \
  pkg-config \
  libssl-dev

# Build and install
cargo install --path .

# Setup systemd service
cat > /etc/systemd/system/sidecar.service << EOF
[Unit]
Description=Sidecar AI Code Assistant
After=network.target

[Service]
Type=simple
User=sidecar
Environment=RUST_LOG=info
ExecStart=/usr/local/bin/sidecar
Restart=always

[Install]
WantedBy=multi-user.target
EOF

systemctl enable --now sidecar
```

## Configuration

### 1. Environment Variables

```bash
# .env
RUST_LOG=info
SIDECAR_PORT=3001
OPENAI_API_KEY=sk-...
ANTHROPIC_API_KEY=sk-...
```

### 2. SSL/TLS Setup

```nginx
# nginx configuration
server {
    listen 443 ssl;
    server_name sidecar.example.com;

    ssl_certificate /etc/letsencrypt/live/sidecar.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/sidecar.example.com/privkey.pem;

    location / {
        proxy_pass http://localhost:3001;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection 'upgrade';
    }
}
```

## Monitoring

### 1. Health Checks

```bash
# Check service health
curl http://localhost:3001/health

# Monitor logs
journalctl -u sidecar -f
```

### 2. Metrics

```yaml
# prometheus.yml
scrape_configs:
  - job_name: 'sidecar'
    static_configs:
      - targets: ['localhost:3001']
```

## Scaling

### 1. Horizontal Scaling

- Use load balancer
- Configure session affinity
- Scale based on CPU/memory usage

### 2. Vertical Scaling

- Increase resources
- Optimize memory usage
- Configure cache size

## Backup and Recovery

1. **Data Backup:**
```bash
# Backup script
#!/bin/bash
DATE=$(date +%Y%m%d)
tar -czf backup-$DATE.tar.gz /data/sidecar
aws s3 cp backup-$DATE.tar.gz s3://backups/
```

2. **Recovery:**
```bash
# Recovery script
#!/bin/bash
aws s3 cp s3://backups/backup-latest.tar.gz .
tar -xzf backup-latest.tar.gz -C /data/sidecar
systemctl restart sidecar
```

## Security

1. **Firewall Rules:**
```bash
# Allow only necessary ports
ufw allow 443/tcp
ufw allow 3001/tcp
```

2. **API Authentication:**
```nginx
# Add authentication header
proxy_set_header Authorization "Bearer ${API_KEY}";
```

## Troubleshooting

1. **Common Issues:**
   - Check logs: `journalctl -u sidecar`
   - Verify permissions
   - Check API keys
   - Monitor resource usage

2. **Performance Issues:**
   - Profile CPU/memory
   - Check network latency
   - Monitor LLM response times

## Maintenance

1. **Updates:**
```bash
# Update script
#!/bin/bash
git pull
cargo build --release
systemctl restart sidecar
```

2. **Cleanup:**
```bash
# Cleanup old logs
find /var/log/sidecar -mtime +30 -delete

# Cleanup temp files
find /tmp/sidecar -mtime +1 -delete
```