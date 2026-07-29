import hashlib
import json
import os
import time

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


class IndexConflict(Exception):
    pass


def fetch_index(client):
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


def merge_index(existing, new_entries, gc_roots, public_key=""):
    existing = existing or {}
    entries = {}
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


def build_index_manifest(index_digest, index_size, config_digest, config_size):
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
                }
            ],
        }
    )


def push_index(client, index, work_dir, if_match=None):
    index_json = json.dumps(index, indent=2, sort_keys=True).encode()
    index_file = os.path.join(work_dir, "cache-index.json")
    with open(index_file, "wb") as f:
        f.write(index_json)

    index_digest = client.push_blob(index_file)
    index_size = os.path.getsize(index_file)

    config_file = os.path.join(work_dir, "config.json")
    with open(config_file, "w") as f:
        f.write("{}")
    config_digest = client.push_blob(config_file)
    config_size = os.path.getsize(config_file)

    manifest = build_index_manifest(index_digest, index_size, config_digest, config_size)
    ok, code = client.push_manifest("cache-index", manifest, if_match=if_match)
    if not ok:
        if code in (409, 412):
            raise IndexConflict(f"cache-index conflict (HTTP {code})")
        raise RuntimeError(f"Failed to push cache-index manifest (HTTP {code})")

    return "sha256:" + hashlib.sha256(manifest.encode()).hexdigest()


def update_index(client, new_entries, gc_roots, public_key="", work_dir=None, max_retries=3):
    if work_dir is None:
        import tempfile

        work_dir = tempfile.mkdtemp(prefix="nixcache-")

    merged = None
    for attempt in range(max_retries):
        existing, old_digest = fetch_index(client)
        merged = merge_index(existing, new_entries, gc_roots, public_key)

        try:
            pushed_digest = push_index(client, merged, work_dir, if_match=old_digest)
        except IndexConflict:
            if attempt < max_retries - 1:
                info(f"Cache index conflict (attempt {attempt + 1}/{max_retries}), retrying...")
                time.sleep(1 + attempt)
                continue
            raise

        _, current_digest = client.get_manifest("cache-index")
        if current_digest == pushed_digest:
            return merged

        if attempt < max_retries - 1:
            info(
                f"Cache index changed after push (attempt {attempt + 1}/{max_retries}), retrying..."
            )
            time.sleep(1 + attempt)
            continue

    err(f"Cache index update failed after {max_retries} attempts")
    assert merged is not None
    return merged
