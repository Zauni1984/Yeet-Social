# YEET Social — VPS Deploy Cheatsheet

## Frontend deploy (instant)
```bash
set +H && curl -sL -H "Authorization: Bearer GITHUB_PAT" \
  "https://api.github.com/repos/Zauni1984/Yeet-Social/contents/frontend/index.html" \
  | python3 -c "import sys,json,base64; d=json.load(sys.stdin); open('/root/yeet-html/index.html','wb').write(base64.b64decode(d['content'].replace('\\n','')))"

# Fix forEach bug after every deploy:
python3 -c "
f=open('/root/yeet-html/index.html','rb').read()
old=b'      wrap.appendChild(div);\n    var composer'
new=b'      wrap.appendChild(div);\n    });\n    var composer'
if old in f:
    open('/root/yeet-html/index.html','wb').write(f.replace(old,new,1))
    print('FIXED')
"
```

## Backend deploy (after CI build)
```bash
bash /tmp/start_backend.sh
```

## Quick health check
```bash
curl -s http://127.0.0.1:8080/api/v1/health
curl -s "http://127.0.0.1:8080/api/v1/link-preview?url=https://hanfjack.de" | python3 -m json.tool
```

## Container status
```bash
docker ps --format "table {{.Names}}\t{{.Status}}\t{{.Ports}}"
```

## Key facts
- Backend listens on **:8080** internally
- Nginx proxies from 80/443 → backend
- Frontend at `/root/yeet-html/index.html`
- Network: `yeet-social_yeet-net`
- Secrets: in `/root/yeet-social/.env` (mode 600, see `vps/.env.example`)

## Secrets / .env (VPS)
`docker-compose.yml` and `start_backend.sh` both expect these at
`/root/yeet-social/.env`:
```
POSTGRES_PASSWORD=...
JWT_SECRET=...
ADMIN_SECRET=...
RUST_LOG=backend=info,tower_http=warn   # optional
```
Generate fresh values with `openssl rand -hex 64` and `openssl rand -hex 32`,
then `chmod 600 /root/yeet-social/.env`. Never commit.

To rotate the JWT secret without breaking active sessions, deploy the new
secret during low-traffic windows  all existing JWTs become invalid and users
must re-login.
