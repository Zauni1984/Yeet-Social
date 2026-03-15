FROM nginx:alpine
RUN echo '<!DOCTYPE html><html><head><meta charset="utf-8"><title>Yeet Social</title><style>body{background:#0d0d0d;color:#fff;font-family:sans-serif;display:flex;align-items:center;justify-content:center;height:100vh;margin:0}.box{text-align:center}.logo{font-size:4rem;margin-bottom:1rem}h1{font-size:2rem;margin:0}p{color:#888}</style></head><body><div class="box"><div class="logo">🚀</div><h1>Yeet Social</h1><p>Frontend coming soon — API is live at /api/v1</p></div></body></html>' > /usr/share/nginx/html/index.html
EXPOSE 80
CMD ["nginx", "-g", "daemon off;"]
