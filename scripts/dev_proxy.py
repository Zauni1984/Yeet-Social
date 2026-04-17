#!/usr/bin/env python3
"""Lokaler Dev-Proxy:
   - serviert ./frontend/ auf http://localhost:5173
   - leitet /api/* an http://localhost:8080 weiter (Rust-Backend)
Start: python3 scripts/dev_proxy.py
"""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.request import Request, urlopen
from urllib.error import HTTPError, URLError
import mimetypes
import os
import sys

FRONTEND_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "frontend")
BACKEND_URL = "http://127.0.0.1:8080"
LISTEN_HOST = "127.0.0.1"
LISTEN_PORT = 5173
HOP_HEADERS = {
    "connection", "keep-alive", "proxy-authenticate", "proxy-authorization",
    "te", "trailers", "transfer-encoding", "upgrade", "host", "content-length",
}


class DevHandler(BaseHTTPRequestHandler):
    def log_message(self, fmt, *args):
        sys.stderr.write("[dev] %s - %s\n" % (self.address_string(), fmt % args))

    def _proxy(self):
        url = BACKEND_URL + self.path
        length = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(length) if length > 0 else None
        req = Request(url, data=body, method=self.command)
        for k, v in self.headers.items():
            if k.lower() not in HOP_HEADERS:
                req.add_header(k, v)
        try:
            resp = urlopen(req, timeout=30)
            self.send_response(resp.status)
            for k, v in resp.getheaders():
                if k.lower() in HOP_HEADERS:
                    continue
                self.send_header(k, v)
            data = resp.read()
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
        except HTTPError as e:
            self.send_response(e.code)
            for k, v in e.headers.items():
                if k.lower() in HOP_HEADERS:
                    continue
                self.send_header(k, v)
            data = e.read() or b""
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
        except URLError as e:
            msg = ("Backend unreachable at %s: %s" % (BACKEND_URL, e)).encode()
            self.send_response(502)
            self.send_header("Content-Type", "text/plain; charset=utf-8")
            self.send_header("Content-Length", str(len(msg)))
            self.end_headers()
            self.wfile.write(msg)

    def _serve_static(self):
        path = self.path.split("?", 1)[0]
        if path == "/" or path == "":
            path = "/index.html"
        fs_path = os.path.normpath(os.path.join(FRONTEND_DIR, path.lstrip("/")))
        if not fs_path.startswith(os.path.abspath(FRONTEND_DIR)):
            self.send_error(403, "Forbidden")
            return
        if not os.path.isfile(fs_path):
            # SPA-Fallback
            fs_path = os.path.join(FRONTEND_DIR, "index.html")
            if not os.path.isfile(fs_path):
                self.send_error(404, "Not Found")
                return
        ctype, _ = mimetypes.guess_type(fs_path)
        with open(fs_path, "rb") as f:
            data = f.read()
        self.send_response(200)
        self.send_header("Content-Type", ctype or "application/octet-stream")
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(data)

    def _handle(self):
        if self.path.startswith("/api/"):
            self._proxy()
        else:
            self._serve_static()

    do_GET = do_POST = do_PUT = do_DELETE = do_PATCH = do_OPTIONS = _handle


def main():
    srv = ThreadingHTTPServer((LISTEN_HOST, LISTEN_PORT), DevHandler)
    print("[dev] frontend: http://%s:%d  (proxying /api -> %s)" % (LISTEN_HOST, LISTEN_PORT, BACKEND_URL))
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        print("\n[dev] shutdown")


if __name__ == "__main__":
    main()
