import hashlib
import os
import sys
from datetime import UTC, datetime

REGISTRY = os.environ.get("NIXCACHE_REGISTRY", "ghcr.io")
REPO = os.environ.get("NIXCACHE_REPO", "")
IMAGE = f"{REGISTRY}/{REPO}/nix-cache" if REPO else ""
UPSTREAM_CACHES = os.environ.get(
    "NIXCACHE_UPSTREAM",
    os.environ.get("NIXCACHE_UPSTREAM_CACHES", "https://cache.nixos.org"),
).split()

INDEX_MEDIA_TYPE = "application/vnd.nix.cache.index.v1+json"
MANIFEST_MEDIA_TYPE = "application/vnd.oci.image.manifest.v1+json"
CONFIG_MEDIA_TYPE = "application/vnd.oci.image.config.v1+json"
STREAM_CHUNK_SIZE = 64 * 1024


def info(msg):
    print(f">>> {msg}", file=sys.stderr)


def err(msg):
    print(f"!!! {msg}", file=sys.stderr)


def utc_now():
    return datetime.now(UTC).strftime("%Y-%m-%dT%H:%M:%SZ")


def fmt_size(n):
    for unit in ("B", "KiB", "MiB", "GiB"):
        if n < 1024:
            return f"{n:.0f}{unit}" if unit == "B" else f"{n:.1f}{unit}"
        n /= 1024
    return f"{n:.1f}TiB"


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def store_hash(store_path):
    return os.path.basename(store_path)[:32]
