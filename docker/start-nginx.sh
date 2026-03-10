#!/bin/sh
cat > /etc/nginx/nginx.conf <<'EOF'
events {
    worker_connections 1024;
}
http {
    server {
        listen 8448 ssl;
        server_name localhost;
        ssl_certificate /etc/nginx/ssl/cert.pem;
        ssl_certificate_key /etc/nginx/ssl/key.pem;
        ssl_protocols TLSv1.2 TLSv1.3;
        location / {
            proxy_pass http://synapse:8008;
            proxy_set_header Host $host;
            proxy_set_header X-Forwarded-For $remote_addr;
            proxy_set_header X-Forwarded-Proto $scheme;
        }
    }
}
EOF
nginx -g 'daemon off;'
