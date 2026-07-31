# Copyright (c) 2026 Simon Berger
# SPDX-License-Identifier: AGPL-3.0-only
"""Configuration and shared utilities for nixcache-oci."""

import hashlib
import os
import sys
from datetime import UTC, datetime
from pathlib import Path

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
NARINFO_MEDIA_TYPE = "text/x-nix-narinfo"
NARINFO_TAG_PREFIX = "ni-"
STREAM_CHUNK_SIZE = 64 * 1024
_KIB = 1024


def info(msg: str) -> None:
    """Log an informational message to stderr."""
    print(f">>> {msg}", file=sys.stderr)


def err(msg: str) -> None:
    """Log an error message to stderr."""
    print(f"!!! {msg}", file=sys.stderr)


def debug(msg: str) -> None:
    """Log a verbose message to stderr, only when Actions step debug logging is on.

    Step debug logging is enabled by the repo secret ACTIONS_STEP_DEBUG,
    which surfaces as RUNNER_DEBUG=1 in every step.
    """
    if os.environ.get("RUNNER_DEBUG") == "1":
        print(f"~~~ {msg}", file=sys.stderr)


def utc_now() -> str:
    """Return the current UTC time as an ISO 8601 string."""
    return datetime.now(UTC).strftime("%Y-%m-%dT%H:%M:%SZ")


def fmt_size(n: float) -> str:
    """Format a byte count with binary units."""
    for unit in ("B", "KiB", "MiB", "GiB"):
        if n < _KIB:
            return f"{n:.0f}{unit}" if unit == "B" else f"{n:.1f}{unit}"
        n /= _KIB
    return f"{n:.1f}TiB"


def sha256_file(path: str | Path) -> str:
    """Return the hex SHA-256 digest of a file."""
    h = hashlib.sha256()
    with Path(path).open("rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def store_hash(store_path: str | Path) -> str:
    """Extract the 32-character hash from a store path."""
    return Path(store_path).name[:32]
