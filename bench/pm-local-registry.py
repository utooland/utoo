#!/usr/bin/env python3
"""Freeze public npm responses for repeatable client-cache benchmarks.

Tarball bytes and integrity are unchanged. Manifest tarball URLs point at this
loopback server. /_pm_bench/{thaw,freeze,reset,stats} control untimed cache fill
and timed offline runs, and expose transferred bytes/request concurrency.
"""

import argparse
import gzip
import hashlib
import http.server
import json
import pathlib
import threading
import urllib.error
import urllib.request


class Registry(http.server.ThreadingHTTPServer):
    daemon_threads = True
    request_queue_size = 256

    def __init__(self, cache, port):
        origin = cache / "origin.json"
        previous = json.loads(origin.read_text()) if origin.exists() else None
        if previous:
            if port and port != previous["port"]:
                raise ValueError("reuse the snapshot's original port or choose a new cache directory")
            port = previous["port"]
        super().__init__(("127.0.0.1", port), Handler)
        self.cache = cache
        self.address = f"http://127.0.0.1:{self.server_port}"
        origin.write_text(json.dumps(dict(port=self.server_port)))
        self.mutex = threading.Lock()
        self.slots = {}
        self.frozen = False
        self.active = 0
        self.tarballs_active = 0
        self.reset()

    def reset(self):
        self.stats = dict(requests=0, bytes=0, tarball_bytes=0, manifest_bytes=0,
                          peak_requests=0, peak_tarballs=0, upstream_requests=0,
                          offline_misses=0)

    def response(self, path):
        key = hashlib.sha256(path.encode()).hexdigest()
        with self.mutex:
            slot = self.slots.setdefault(key, threading.Lock())
        with slot:
            data = self.cache / f"{key}.body"
            metadata = self.cache / f"{key}.json"
            if data.exists() and metadata.exists():
                return json.loads(metadata.read_text()), data.read_bytes()
            with self.mutex:
                if self.frozen:
                    self.stats["offline_misses"] += 1
                    raise RuntimeError(f"uncached request in frozen registry: {path}")
                self.stats["upstream_requests"] += 1
            request = urllib.request.Request(
                "https://registry.npmjs.org" + path,
                headers={"Accept": "application/json", "Accept-Encoding": "identity"},
            )
            try:
                response = urllib.request.urlopen(request, timeout=120)
            except urllib.error.HTTPError as error:
                response = error
            with response:
                body = response.read()
                status = response.status
                content_type = response.headers.get("Content-Type", "application/octet-stream")
            encoding = None
            if status == 200 and "json" in content_type:
                manifest = json.loads(body)
                versions = [manifest, *manifest.get("versions", {}).values()]
                for version in versions:
                    if not isinstance(version, dict):
                        continue
                    dist = version.get("dist", {})
                    if not isinstance(dist, dict):
                        continue
                    url = dist.get("tarball", "")
                    if not isinstance(url, str):
                        continue
                    for origin in ("https://registry.npmjs.org", "http://registry.npmjs.org"):
                        if url.startswith(origin + "/"):
                            dist["tarball"] = self.address + url[len(origin):]
                            break
                body = gzip.compress(json.dumps(manifest, separators=(",", ":")).encode(), mtime=0)
                encoding = "gzip"
            meta = dict(status=status, content_type=content_type, encoding=encoding,
                        etag='"' + hashlib.sha256(body).hexdigest() + '"')
            # Slot lock serializes all readers; a crash before both writes is a miss.
            data.write_bytes(body)
            metadata.write_text(json.dumps(meta))
            return meta, body


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_args):
        pass

    def send(self, status, body, headers=()):
        self.send_response(status)
        self.send_header("Content-Length", str(len(body)))
        for key, value in headers:
            self.send_header(key, value)
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        server = self.server
        if self.path.startswith("/_pm_bench/"):
            operation = self.path.rsplit("/", 1)[-1]
            with server.mutex:
                if operation == "reset":
                    if server.active:
                        self.send(409, b"requests still active")
                        return
                    server.reset()
                elif operation == "freeze":
                    server.frozen = True
                elif operation == "thaw":
                    server.frozen = False
                body = json.dumps(dict(server.stats, active=server.active, frozen=server.frozen)).encode()
            self.send(200, body, [("Content-Type", "application/json")])
            return

        tarball = self.path.split("?", 1)[0].endswith(".tgz")
        with server.mutex:
            server.active += 1
            server.tarballs_active += int(tarball)
            server.stats["requests"] += 1
            server.stats["peak_requests"] = max(server.stats["peak_requests"], server.active)
            server.stats["peak_tarballs"] = max(server.stats["peak_tarballs"], server.tarballs_active)
        try:
            meta, body = server.response(self.path)
            status = meta["status"]
            if self.headers.get("If-None-Match") == meta["etag"] and status == 200:
                status, body = 304, b""
            headers = [("Content-Type", meta["content_type"]), ("ETag", meta["etag"])]
            if meta["encoding"]:
                headers.append(("Content-Encoding", meta["encoding"]))
            self.send(status, body, headers)
            with server.mutex:
                server.stats["bytes"] += len(body)
                server.stats["tarball_bytes" if tarball else "manifest_bytes"] += len(body)
        except (BrokenPipeError, ConnectionResetError):
            pass
        except Exception as error:
            self.send(502, str(error).encode())
        finally:
            with server.mutex:
                server.active -= 1
                server.tarballs_active -= int(tarball)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", type=pathlib.Path, required=True)
    parser.add_argument("--port", type=int, default=0)
    parser.add_argument("--address-file", type=pathlib.Path, required=True)
    args = parser.parse_args()
    args.cache.mkdir(parents=True, exist_ok=True)
    registry = Registry(args.cache, args.port)
    args.address_file.write_text(registry.address)
    print(registry.address, flush=True)
    registry.serve_forever()
