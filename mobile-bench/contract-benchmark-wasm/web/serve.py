#!/usr/bin/env python3
"""
Static server for the contract-benchmark-wasm web bench, with a
same-origin proxy for SRS params.

`srs.midnight.network` doesn't set `Access-Control-Allow-Origin`,
so browsers block direct cross-origin fetches from
`http://localhost:8080`. This server forwards `/srs/<file>` to
`https://srs.midnight.network/<file>` with the body streamed back
verbatim — keeps the JS-side fetch URL same-origin.

Usage:  python3 serve.py [port]   (default port 8080)
"""

import http.server
import socketserver
import sys
import urllib.request
import urllib.error

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8080
SRS_ORIGIN = "https://srs.midnight.network"
ALLOWED_FILES_PREFIX = "bls_midnight_2p"


class Handler(http.server.SimpleHTTPRequestHandler):
    """Static files + /srs/<file> proxy."""

    def do_GET(self):  # noqa: N802
        if self.path.startswith("/srs/"):
            self._proxy_srs(self.path[len("/srs/"):])
            return
        super().do_GET()

    def end_headers(self):  # noqa: D102
        # Dev-server only — disable browser caching of the page /
        # wasm assets so reload always picks up the latest build.
        # SRS files served via `_proxy_srs` set their own cache
        # header and skip this path.
        self.send_header("Cache-Control", "no-store, no-cache, must-revalidate")
        # SharedArrayBuffer (and therefore `wasm-bindgen-rayon`'s
        # worker pool) is only available when the page is
        # cross-origin-isolated. That requires both of these
        # headers on every response. Without them, rayon falls
        # back to its single-threaded no-pool mode and the
        # threading patch is invisible.
        self.send_header("Cross-Origin-Opener-Policy", "same-origin")
        self.send_header("Cross-Origin-Embedder-Policy", "require-corp")
        # COEP=require-corp means every subresource must opt in,
        # so the wasm + JS + SRS bytes the proxy serves all need
        # this. Same-origin assets are eligible.
        self.send_header("Cross-Origin-Resource-Policy", "same-origin")
        super().end_headers()

    def _proxy_srs(self, filename: str) -> None:
        # Belt-and-braces: only proxy the exact files we expect.
        # No `../` traversal, no arbitrary URLs.
        if not filename.startswith(ALLOWED_FILES_PREFIX) or "/" in filename:
            self.send_error(400, f"refused: {filename!r}")
            return
        url = f"{SRS_ORIGIN}/{filename}"
        try:
            with urllib.request.urlopen(url, timeout=30) as upstream:
                self.send_response(upstream.status)
                # Pass through length + content-type; everything else
                # we'd want is `Access-Control-Allow-Origin` which is
                # the whole point of doing this proxy.
                self.send_header(
                    "Content-Type",
                    upstream.headers.get("Content-Type", "application/octet-stream"),
                )
                length = upstream.headers.get("Content-Length")
                if length is not None:
                    self.send_header("Content-Length", length)
                self.send_header("Access-Control-Allow-Origin", "*")
                self.send_header("Cache-Control", "public, max-age=86400")
                self.end_headers()
                # Stream in 1 MiB chunks so large SRS files don't
                # buffer the whole body in memory before flushing.
                while True:
                    chunk = upstream.read(1024 * 1024)
                    if not chunk:
                        break
                    self.wfile.write(chunk)
        except urllib.error.HTTPError as e:
            self.send_error(e.code, f"upstream: {e.reason}")
        except urllib.error.URLError as e:
            self.send_error(502, f"upstream unreachable: {e.reason}")


class ReusableTCPServer(socketserver.TCPServer):
    allow_reuse_address = True


if __name__ == "__main__":
    print(f"serving http://localhost:{PORT}/  (SRS proxy at /srs/<file>)")
    with ReusableTCPServer(("", PORT), Handler) as httpd:
        try:
            httpd.serve_forever()
        except KeyboardInterrupt:
            print()
