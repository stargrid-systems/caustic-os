# Copyright (c) 2026 Simon Berger
# SPDX-License-Identifier: AGPL-3.0-only
"""Cache-index model with optimistic concurrency."""

import hashlib
import json
import tempfile
import time
from pathlib import Path
from typing import Any

from nixcache.config import (
    CONFIG_MEDIA_TYPE,
    IMAGE,
    INDEX_MEDIA_TYPE,
    MANIFEST_MEDIA_TYPE,
    REGISTRY,
    REPO,
    err,
    info,
    utc_now,
)
from nixcache.oci import OCIClient

_HTTP_CONFLICT = 409
_HTTP_PRECONDITION_FAILED = 412
_INDEX_MAX_RETRIES = 3


class IndexConflictError(Exception):
    """Raised when the cache-index was modified concurrently."""


def fetch_index(client: OCIClient) -> tuple[dict[str, Any] | None, str | None]:
    """Fetch and parse the cache-index from OCI, returning (index, manifest_digest)."""
    manifest_data, digest = client.get_manifest("cache-index")
    if manifest_data is None:
        return None, None
    try:
        manifest = json.loads(manifest_data)
        layers = manifest.get("layers", [])
        if not layers:
            return None, digest
        index_digest = layers[0]["digest"]
        index_data = client.get_blob(index_digest)
        if index_data is None:
            err("cache-index blob missing or unreachable")
            return None, digest
        return json.loads(index_data), digest
    except (json.JSONDecodeError, KeyError) as e:
        err(f"Failed to parse cache-index: {e}")
        return None, digest


def merge_index(
    existing: dict[str, Any] | None,
    new_entries: dict[str, Any],
    gc_roots: list[str],
    public_key: str = "",
) -> dict[str, Any]:
    """Merge new entries and GC roots into an existing index."""
    existing = existing or {}
    entries: dict[str, Any] = {}
    entries.update(existing.get("entries", {}))
    entries.update(new_entries)
    return {
        "version": 1,
        "repo": REPO,
        "registry": REGISTRY,
        "image": IMAGE,
        "generated": utc_now(),
        "public_key": public_key or existing.get("public_key", ""),
        "entries": entries,
        "gc_roots": sorted(set(existing.get("gc_roots", [])) | set(gc_roots)),
    }


def build_index_manifest(
    index_digest: str,
    index_size: int,
    config_digest: str,
    config_size: int,
) -> str:
    """Build the OCI manifest JSON for the cache-index."""
    return json.dumps(
        {
            "schemaVersion": 2,
            "mediaType": MANIFEST_MEDIA_TYPE,
            "config": {
                "mediaType": CONFIG_MEDIA_TYPE,
                "digest": config_digest,
                "size": config_size,
            },
            "layers": [
                {
                    "mediaType": INDEX_MEDIA_TYPE,
                    "digest": index_digest,
                    "size": index_size,
                },
            ],
        },
    )


def push_index(
    client: OCIClient,
    index: dict[str, Any],
    work_dir: str,
    if_match: str | None = None,
) -> str:
    """Push the cache-index as OCI blobs + manifest, returning the manifest digest."""
    index_json = json.dumps(index, indent=2, sort_keys=True).encode()
    index_file = Path(work_dir) / "cache-index.json"
    index_file.write_bytes(index_json)

    index_digest = client.push_blob(str(index_file))
    index_size = index_file.stat().st_size

    config_file = Path(work_dir) / "config.json"
    config_file.write_text("{}")
    config_digest = client.push_blob(str(config_file))
    config_size = config_file.stat().st_size

    manifest = build_index_manifest(index_digest, index_size, config_digest, config_size)
    ok, code = client.push_manifest("cache-index", manifest, if_match=if_match)
    if not ok:
        if code in (_HTTP_CONFLICT, _HTTP_PRECONDITION_FAILED):
            msg = f"cache-index conflict (HTTP {code})"
            raise IndexConflictError(msg)
        msg = f"Failed to push cache-index manifest (HTTP {code})"
        raise RuntimeError(msg)

    return "sha256:" + hashlib.sha256(manifest.encode()).hexdigest()


def update_index(
    client: OCIClient,
    new_entries: dict[str, Any],
    gc_roots: list[str],
    *,
    public_key: str = "",
    work_dir: str | None = None,
) -> dict[str, Any]:
    """Update the cache-index with optimistic concurrency, retrying on conflict."""
    if work_dir is None:
        work_dir = tempfile.mkdtemp(prefix="nixcache-")

    merged: dict[str, Any] | None = None
    for attempt in range(_INDEX_MAX_RETRIES):
        existing, old_digest = fetch_index(client)
        merged = merge_index(existing, new_entries, gc_roots, public_key)

        try:
            pushed_digest = push_index(client, merged, work_dir, if_match=old_digest)
        except IndexConflictError:
            if attempt < _INDEX_MAX_RETRIES - 1:
                info(
                    f"Cache index conflict (attempt {attempt + 1}/{_INDEX_MAX_RETRIES}),"
                    " retrying...",
                )
                time.sleep(1 + attempt)
                continue
            raise

        _, current_digest = client.get_manifest("cache-index")
        if current_digest == pushed_digest:
            return merged

        if attempt < _INDEX_MAX_RETRIES - 1:
            info(
                f"Cache index changed after push (attempt {attempt + 1}/{_INDEX_MAX_RETRIES}),"
                " retrying...",
            )
            time.sleep(1 + attempt)
            continue

    err(f"Cache index update failed after {_INDEX_MAX_RETRIES} attempts")
    if merged is None:
        msg = "Cache index update produced no result"
        raise RuntimeError(msg)
    return merged
