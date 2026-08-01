# Copyright (c) 2026 Simon Berger
# SPDX-License-Identifier: AGPL-3.0-only
"""Upload locally-built Nix store paths to the OCI cache."""

import argparse
import os
import random
import sys
import tempfile
from collections import Counter
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path
from typing import Any

from nixcache.config import debug, err, fmt_size, info, store_hash, utc_now
from nixcache.index import push_narinfo_manifest, update_index
from nixcache.nar import (
    dump_nars,
    find_locally_built_paths,
    nar_self_check,
)
from nixcache.oci import OCIClient

_MAX_WORKERS = 8


def _upload_nars(
    client: OCIClient,
    entries: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    """Upload NAR blobs to GHCR in parallel, returning successful entries."""
    info(f"Uploading {len(entries)} NAR blobs to GHCR ({_MAX_WORKERS} parallel)")

    def upload_one(entry: dict[str, Any]) -> dict[str, Any]:
        entry["nar_digest"] = client.push_blob(entry["nar_file"])
        return entry

    uploaded: list[dict[str, Any]] = []
    failures = 0
    with ThreadPoolExecutor(max_workers=_MAX_WORKERS) as pool:
        futures = {pool.submit(upload_one, e): e for e in entries}
        for future in as_completed(futures):
            entry = futures[future]
            try:
                result = future.result()
                uploaded.append(result)
                debug(f"Uploaded {result['hash']} ({fmt_size(result['nar_size'])})")
            except (RuntimeError, OSError) as e:
                failures += 1
                debug(f"Failed to upload NAR for {entry['hash']}: {e}")

    if failures:
        err(f"{failures} upload(s) failed, continuing with {len(uploaded)} successful")

    return uploaded


def _push_narinfo_manifests(
    client: OCIClient,
    entries: list[dict[str, Any]],
    work_dir: str,
) -> None:
    """Push per-hash narinfo manifests in parallel, logging an aggregate summary.

    Per-path detail is emitted only when Actions step debug logging is on.
    """
    info(f"Pushing {len(entries)} per-hash narinfo manifests ({_MAX_WORKERS} parallel)")

    def push_ni(entry: dict[str, Any]) -> tuple[bool, int]:
        return push_narinfo_manifest(
            client,
            entry["hash"],
            entry["text"],
            entry["nar_digest"],
            work_dir,
        )

    ni_ok = 0
    fail_by_code: Counter[int] = Counter()
    with ThreadPoolExecutor(max_workers=_MAX_WORKERS) as pool:
        futures = {pool.submit(push_ni, e): e for e in entries}
        for future in as_completed(futures):
            entry = futures[future]
            try:
                ok, code = future.result()
            except (RuntimeError, OSError) as e:
                ok, code = False, 0
                debug(f"narinfo manifest push error for {entry['hash']}: {e}")
            if ok:
                ni_ok += 1
            else:
                fail_by_code[code] += 1
                debug(f"narinfo manifest for {entry['hash']} failed (HTTP {code})")

    ni_fail = sum(fail_by_code.values())
    if ni_fail:
        detail = ", ".join(f"HTTP {c} x{n}" for c, n in sorted(fail_by_code.items()))
        err(f"Pushed {ni_ok}/{len(entries)} narinfo manifests ({ni_fail} failed: {detail})")
    else:
        info(f"Pushed {ni_ok}/{len(entries)} narinfo manifests")


def _build_index_entries(entries: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    """Build cache-index entries from parsed narinfo data."""
    new_entries: dict[str, dict[str, Any]] = {}
    for e in entries:
        store_path = e.get("storepath", "")
        name = Path(store_path).name.split("-", 1)[-1] if store_path else e["hash"]
        new_entries[e["hash"]] = {
            "name": name,
            "narinfo": e["text"],
            "nar_digest": e["nar_digest"],
            "nar_size": e["nar_size"],
            "added": utc_now(),
        }
    return new_entries


def _read_public_key(signing_key: str) -> str:
    """Read the public key from env var or the .pub file alongside the signing key."""
    env_key = os.environ.get("NIXCACHE_PUBLIC_KEY", "")
    if env_key:
        return env_key
    if not signing_key:
        return ""
    pub_path = Path(f"{signing_key}.pub")
    if pub_path.exists():
        return pub_path.read_text().strip()
    return ""


def _resolve_gc_root(out_link: str) -> list[str]:
    """Resolve the out-link symlink to a GC root hash."""
    link = Path(out_link)
    if link.is_symlink() or link.exists():
        top_level = str(link.resolve())
        if top_level.startswith("/nix/store/"):
            return [store_hash(top_level)]
    return []


def main() -> int:
    """Entry point for nixcache-upload."""
    parser = argparse.ArgumentParser(
        description="Upload locally-built Nix store paths to the OCI cache",
    )
    parser.add_argument(
        "--out-link",
        default="result",
        help="Nix output symlink to record as a GC root (default: result)",
    )
    parser.add_argument(
        "--force",
        action="store_true",
        help="Re-upload all non-baseline paths, ignoring index. Use to repair missing NAR blobs.",
    )
    args = parser.parse_args()

    client = OCIClient(push=True)
    signing_key = os.environ.get("NIXCACHE_SIGNING_KEY_FILE", "")

    upload_list = find_locally_built_paths(client, force=args.force)
    if not upload_list:
        info("Nothing to upload. Store has no locally-built paths")
        return 0

    info(f"Found {len(upload_list)} locally-built paths to export")

    work_dir = tempfile.mkdtemp(prefix="nixcache-")
    cache_dir = Path(work_dir) / "cache"

    entries = dump_nars(upload_list, cache_dir, signing_key)
    if not entries:
        info("No paths dumped successfully")
        return 0

    info(f"Dumped {len(entries)} paths")

    sample = random.choice(entries)
    try:
        nar_self_check(sample)
    except (RuntimeError, OSError) as e:
        err(f"ABORTING: {e}")
        return 1

    public_key = _read_public_key(signing_key)

    entries = _upload_nars(client, entries)
    if not entries:
        err("All NAR uploads failed. Cache was not updated.")
        return 1

    gc_roots = _resolve_gc_root(args.out_link)
    new_entries = _build_index_entries(entries)

    _push_narinfo_manifests(client, entries, work_dir)

    info(f"Uploaded {len(entries)} NAR(s), updating index")
    index = update_index(client, new_entries, gc_roots, public_key=public_key, work_dir=work_dir)
    info(
        f"Cache index pushed to GHCR"
        f" ({len(index['entries'])} total entries, {len(new_entries)} new)",
    )

    return 0


if __name__ == "__main__":
    sys.exit(main())
