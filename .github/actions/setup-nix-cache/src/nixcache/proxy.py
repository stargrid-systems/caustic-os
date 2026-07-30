# Copyright (c) 2026 Simon Berger
# SPDX-License-Identifier: AGPL-3.0-only
"""HTTP proxy bridging the Nix binary cache protocol to GHCR OCI."""

import contextlib
import http.server
import json
import os
import signal
import sys
import threading
import time
from http.client import HTTPResponse
from pathlib import Path
from typing import Any

from nixcache.config import REPO, STREAM_CHUNK_SIZE, UPSTREAM_CACHES
from nixcache.index import fetch_index
from nixcache.nar import sanitize_narinfo
from nixcache.oci import OCIClient, fetch_url, open_stream

PORT = int(os.environ.get("NIXCACHE_PORT", "37515"))
LISTEN_ADDR = os.environ.get("NIXCACHE_LISTEN", "127.0.0.1")
INDEX_TTL = int(os.environ.get("NIXCACHE_INDEX_TTL", "300"))


def _default_index_dir() -> Path:
    """Determine the directory for the cached index file."""
    explicit = os.environ.get("NIXCACHE_INDEX_DIR")
    if explicit:
        return Path(explicit)
    cache_dir = os.environ.get("CACHE_DIRECTORY")
    if cache_dir:
        return Path(cache_dir)
    return Path.home() / ".cache" / "nixcache-proxy" / REPO.replace("/", "--")


INDEX_DIR = _default_index_dir()

client = OCIClient(push=False)


class CacheIndex:
    """Thread-safe cache-index with TTL-based refresh."""

    def __init__(self) -> None:
        """Initialize the cache index with empty state."""
        self._index: dict[str, Any] | None = None
        self._nar_map: dict[str, str] = {}
        self._lock = threading.Lock()
        self._last_fetch = 0.0
        self._index_file = INDEX_DIR / "cache-index.json"

    def get(self) -> dict[str, Any]:
        """Return the current index, refreshing if stale."""
        with self._lock:
            if time.time() - self._last_fetch > INDEX_TTL:
                self._refresh()
            return self._index or {"entries": {}, "gc_roots": []}

    def force_refresh(self) -> int:
        """Force an immediate refresh, returning the entry count."""
        with self._lock:
            self._last_fetch = 0.0
            self._refresh()
            entries = self._index.get("entries", {}) if self._index else {}
            return len(entries)

    def _refresh(self) -> None:
        index, digest = fetch_index(client)
        if index:
            self._index = index
            self._index_file.parent.mkdir(parents=True, exist_ok=True)
            self._index_file.write_bytes(json.dumps(index).encode())
            print(
                f"[nixcache-proxy] Index refreshed: {len(index.get('entries', {}))} entries",
                file=sys.stderr,
            )
        elif digest:
            print(
                "[nixcache-proxy] Index manifest exists but content is missing or corrupt",
                file=sys.stderr,
            )

        if not self._index and self._index_file.exists():
            with contextlib.suppress(json.JSONDecodeError):
                self._index = json.loads(self._index_file.read_bytes())

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

    def lookup(self, store_hash: str) -> dict[str, Any] | None:
        """Look up a store hash in the index."""
        index = self.get()
        return index.get("entries", {}).get(store_hash)

    def find_nar_digest(self, nar_basename: str) -> str | None:
        """Find the OCI blob digest for a NAR basename."""
        self.get()
        return self._nar_map.get(nar_basename)


cache_index = CacheIndex()


def get_nci_response() -> bytes:
    """Return the nix-cache-info response body."""
    lines = [
        "StoreDir: /nix/store",
        "WantMassQuery: 1",
        "Priority: 40",
    ]
    return "\n".join(lines).encode() + b"\n"


def upstream_stream_nar(
    path: str,
) -> tuple[HTTPResponse | None, int | None]:
    """Try upstream caches for a NAR, returning (response, content_length)."""
    for cache_url in UPSTREAM_CACHES:
        resp, length = open_stream(f"{cache_url}{path}", timeout=60)
        if resp is not None:
            return resp, length
    return None, None


class CacheHandler(http.server.BaseHTTPRequestHandler):
    """HTTP request handler for the Nix binary cache proxy."""

    def log_message(self, format: str, *args: object) -> None:  # noqa: A002
        """Write a log message to stderr."""
        sys.stderr.write(f"[nixcache-proxy] {format % args}\n")

    def do_GET(self) -> None:
        """Handle GET requests."""
        self.head_only = False
        self._route()

    def do_HEAD(self) -> None:
        """Handle HEAD requests."""
        self.head_only = True
        self._route()

    def _route(self) -> None:
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

    def do_POST(self) -> None:
        """Handle POST requests."""
        path = self.path.rstrip("/")
        if path == "/_refresh":
            self._handle_refresh()
        else:
            self.send_error(404)

    def _serve_bytes(self, data: bytes, content_type: str) -> None:
        self.send_response(200)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        if not getattr(self, "head_only", False):
            self.wfile.write(data)

    def _stream_response(
        self,
        resp: HTTPResponse,
        content_length: int | None,
        content_type: str,
    ) -> None:
        try:
            self.send_response(200)
            self.send_header("Content-Type", content_type)
            if content_length and content_length > 0:
                self.send_header("Content-Length", str(content_length))
            self.end_headers()
            if getattr(self, "head_only", False):
                return
            while True:
                chunk = resp.read(STREAM_CHUNK_SIZE)
                if not chunk:
                    break
                self.wfile.write(chunk)
        except (BrokenPipeError, ConnectionResetError):
            sys.stderr.write("[nixcache-proxy] client disconnected during stream\n")

    def _serve_public_key(self) -> None:
        index = cache_index.get()
        pk = index.get("public_key", "")
        if pk:
            self._serve_bytes(pk.encode() + b"\n", "text/plain")
        else:
            self.send_error(404, "No public key configured")

    def _serve_status(self) -> None:
        index = cache_index.get()
        status = {
            "index_entries": len(index.get("entries", {})),
            "index_generated": index.get("generated", "unknown"),
            "index_ttl": INDEX_TTL,
            "repo": REPO,
            "upstream": UPSTREAM_CACHES,
        }
        body = json.dumps(status, indent=2).encode() + b"\n"
        self._serve_bytes(body, "application/json")

    def _handle_refresh(self) -> None:
        count = cache_index.force_refresh()
        body = json.dumps({"refreshed": True, "entries": count}).encode() + b"\n"
        self._serve_bytes(body, "application/json")

    def _serve_narinfo(self, path: str) -> None:
        store_hash = path.lstrip("/").removesuffix(".narinfo")

        entry = cache_index.lookup(store_hash)
        if entry and "narinfo" in entry:
            body = sanitize_narinfo(entry["narinfo"]).encode("utf-8")
            self._serve_bytes(body, "text/x-nix-narinfo")
            return

        for cache_url in UPSTREAM_CACHES:
            data = fetch_url(f"{cache_url}/{store_hash}.narinfo", timeout=10)
            if data is not None:
                self._serve_bytes(data, "text/x-nix-narinfo")
                return

        self.send_error(404)

    def _serve_nar(self, path: str) -> None:
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


def main() -> None:
    """Entry point for nixcache-proxy."""
    print(f"nixcache-proxy starting on http://{LISTEN_ADDR}:{PORT}", file=sys.stderr)
    print(f"  Repo: {REPO}", file=sys.stderr)
    print(f"  Upstream: {', '.join(UPSTREAM_CACHES)}", file=sys.stderr)
    print(f"  Index TTL: {INDEX_TTL}s", file=sys.stderr)

    server = http.server.ThreadingHTTPServer((LISTEN_ADDR, PORT), CacheHandler)

    threading.Thread(target=cache_index.get, daemon=True).start()

    def shutdown(_signum: int, _frame: object) -> None:
        print("\nShutting down...", file=sys.stderr)
        threading.Thread(target=server.shutdown, daemon=True).start()

    signal.signal(signal.SIGINT, shutdown)
    signal.signal(signal.SIGTERM, shutdown)

    server.serve_forever()


if __name__ == "__main__":
    main()
