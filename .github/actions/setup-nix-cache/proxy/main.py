#!/usr/bin/env python3
"""
nixcache-proxy — Local HTTP proxy bridging Nix binary cache protocol to GHCR.

Serves narinfo responses from a locally-cached index (zero network latency).
Streams NAR blobs directly from GHCR or upstream caches to Nix.
"""

import http.server
import json
import os
import signal
import sys
import threading
import time
from pathlib import Path

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "lib"))
import nixcache
from nixcache import OCIClient, fetch_index, fetch_url, open_stream

PORT = int(os.environ.get("NIXCACHE_PORT", "37515"))
LISTEN_ADDR = os.environ.get("NIXCACHE_LISTEN", "127.0.0.1")
INDEX_TTL = int(os.environ.get("NIXCACHE_INDEX_TTL", "300"))
UPSTREAM_CACHES = nixcache.UPSTREAM_CACHES
STREAM_CHUNK_SIZE = nixcache.STREAM_CHUNK_SIZE


def _default_index_dir():
    explicit = os.environ.get("NIXCACHE_INDEX_DIR")
    if explicit:
        return Path(explicit)
    cache_dir = os.environ.get("CACHE_DIRECTORY")
    if cache_dir:
        return Path(cache_dir)
    return Path.home() / ".cache" / "nixcache-proxy" / nixcache.REPO.replace("/", "--")


INDEX_DIR = _default_index_dir()

client = OCIClient(push=False)


# ── Index ─────────────────────────────────────────────────────────────

class CacheIndex:
    def __init__(self):
        self._index = None
        self._nar_map = {}
        self._lock = threading.Lock()
        self._last_fetch = 0.0
        self._index_file = INDEX_DIR / "cache-index.json"

    def get(self):
        with self._lock:
            if time.time() - self._last_fetch > INDEX_TTL:
                self._refresh()
            return self._index or {"entries": {}, "gc_roots": []}

    def force_refresh(self):
        with self._lock:
            self._last_fetch = 0.0
            self._refresh()
            entries = self._index.get("entries", {}) if self._index else {}
            return len(entries)

    def _refresh(self):
        index, _ = fetch_index(client)
        if index:
            self._index = index
            self._index_file.parent.mkdir(parents=True, exist_ok=True)
            self._index_file.write_bytes(json.dumps(index).encode())
            print(
                f"[nixcache-proxy] Index refreshed: "
                f"{len(index.get('entries', {}))} entries",
                file=sys.stderr,
            )

        if not self._index and self._index_file.exists():
            try:
                self._index = json.loads(self._index_file.read_bytes())
            except json.JSONDecodeError:
                pass

        self._nar_map = {}
        if self._index:
            for entry in self._index.get("entries", {}).values():
                nar_digest = entry.get("nar_digest")
                if not nar_digest:
                    continue
                for line in entry.get("narinfo", "").split("\n"):
                    if line.startswith("URL: "):
                        self._nar_map[line[5:].strip()] = nar_digest
                        break

        self._last_fetch = time.time()

    def lookup(self, store_hash):
        index = self.get()
        return index.get("entries", {}).get(store_hash)

    def find_nar_digest(self, nar_basename):
        self.get()
        return self._nar_map.get(f"nar/{nar_basename}")


cache_index = CacheIndex()


# ── HTTP handler ──────────────────────────────────────────────────────

def get_nci_response():
    lines = [
        "StoreDir: /nix/store",
        "WantMassQuery: 1",
        "Priority: 40",
    ]
    return "\n".join(lines).encode() + b"\n"


def upstream_stream_nar(path):
    for cache_url in UPSTREAM_CACHES:
        resp, length = open_stream(f"{cache_url}{path}", timeout=60)
        if resp is not None:
            return resp, length
    return None, 0


class CacheHandler(http.server.BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        sys.stderr.write(f"[nixcache-proxy] {format % args}\n")

    def do_GET(self):
        self.head_only = False
        self._route()

    def do_HEAD(self):
        self.head_only = True
        self._route()

    def _route(self):
        path = self.path.rstrip("/")
        if path == "/nix-cache-info":
            self._serve_bytes(get_nci_response(), "text/x-nix-cache-info")
        elif path == "/public-key":
            self._serve_public_key()
        elif path == "/_status":
            self._serve_status()
        elif path.endswith(".narinfo"):
            self._serve_narinfo(path)
        elif path.startswith("/nar/"):
            self._serve_nar(path)
        else:
            self.send_error(404)

    def do_POST(self):
        path = self.path.rstrip("/")
        if path == "/_refresh":
            self._handle_refresh()
        else:
            self.send_error(404)

    def _serve_bytes(self, data, content_type):
        self.send_response(200)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        if not getattr(self, "head_only", False):
            self.wfile.write(data)

    def _stream_response(self, resp, content_length, content_type):
        self.send_response(200)
        self.send_header("Content-Type", content_type)
        if content_length is not None:
            self.send_header("Content-Length", str(content_length))
        self.end_headers()
        if getattr(self, "head_only", False):
            resp.close()
            return
        while True:
            chunk = resp.read(STREAM_CHUNK_SIZE)
            if not chunk:
                break
            self.wfile.write(chunk)

    def _serve_public_key(self):
        index = cache_index.get()
        pk = index.get("public_key", "")
        if pk:
            self._serve_bytes(pk.encode() + b"\n", "text/plain")
        else:
            self.send_error(404, "No public key configured")

    def _serve_status(self):
        index = cache_index.get()
        status = {
            "index_entries": len(index.get("entries", {})),
            "index_generated": index.get("generated", "unknown"),
            "index_ttl": INDEX_TTL,
            "repo": nixcache.REPO,
            "upstream": UPSTREAM_CACHES,
        }
        body = json.dumps(status, indent=2).encode() + b"\n"
        self._serve_bytes(body, "application/json")

    def _handle_refresh(self):
        count = cache_index.force_refresh()
        body = json.dumps({"refreshed": True, "entries": count}).encode() + b"\n"
        self._serve_bytes(body, "application/json")

    def _serve_narinfo(self, path):
        store_hash = path.lstrip("/").removesuffix(".narinfo")

        entry = cache_index.lookup(store_hash)
        if entry and "narinfo" in entry:
            body = entry["narinfo"].encode("utf-8")
            self._serve_bytes(body, "text/x-nix-narinfo")
            return

        for cache_url in UPSTREAM_CACHES:
            data = fetch_url(f"{cache_url}/{store_hash}.narinfo", timeout=10)
            if data is not None:
                self._serve_bytes(data, "text/x-nix-narinfo")
                return

        self.send_error(404)

    def _serve_nar(self, path):
        nar_basename = path.removeprefix("/nar/")
        ct = "application/x-xz" if nar_basename.endswith(".xz") else "application/x-nix-nar"

        nar_digest = cache_index.find_nar_digest(nar_basename)
        if nar_digest:
            resp, length = client.stream_blob(nar_digest)
            if resp is not None:
                self._stream_response(resp, length, ct)
                resp.close()
                return

        resp, length = upstream_stream_nar(path)
        if resp is not None:
            self._stream_response(resp, length, ct)
            resp.close()
            return

        self.send_error(404)


def main():
    print(f"nixcache-proxy starting on http://{LISTEN_ADDR}:{PORT}", file=sys.stderr)
    print(f"  Repo: {nixcache.REPO}", file=sys.stderr)
    print(f"  Upstream: {', '.join(UPSTREAM_CACHES)}", file=sys.stderr)
    print(f"  Index TTL: {INDEX_TTL}s", file=sys.stderr)

    server = http.server.ThreadingHTTPServer((LISTEN_ADDR, PORT), CacheHandler)

    threading.Thread(target=cache_index.get, daemon=True).start()

    def shutdown(signum, frame):
        print("\nShutting down...", file=sys.stderr)
        threading.Thread(target=server.shutdown, daemon=True).start()

    signal.signal(signal.SIGINT, shutdown)
    signal.signal(signal.SIGTERM, shutdown)

    server.serve_forever()


if __name__ == "__main__":
    main()
