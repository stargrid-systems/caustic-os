# Copyright (c) 2026 Simon Berger
# SPDX-License-Identifier: AGPL-3.0-only
"""Upload locally-built Nix store paths to the OCI cache."""

import argparse
import os
import random
import sys
import tempfile
from pathlib import Path
from typing import Any

from nixcache.config import err, fmt_size, info, store_hash, utc_now
from nixcache.index import update_index
from nixcache.nar import export_path, find_locally_built_paths, nar_self_check, sign_paths
from nixcache.oci import OCIClient


def _export_all(
    upload_list: list[str],
    cache_dir: Path,
) -> list[dict[str, Any]]:
    """Export store paths as NARs, returning receipts."""
    info(f"Exporting {len(upload_list)} store paths (direct NAR dump, skipping full closure)")
    receipts = []
    for store_path in upload_list:
        try:
            result = export_path(store_path, cache_dir)
            info(f"  Exported {result['hash']} ({fmt_size(result['nar_size'])})")
            receipts.append(result)
        except (RuntimeError, OSError) as e:
            err(f"Failed to export {store_path}: {e}")
    return receipts


def _upload_nars(
    client: OCIClient,
    receipts: list[dict[str, Any]],
) -> tuple[list[dict[str, Any]], int, int]:
    """Upload NAR blobs, returning (successful_receipts, uploaded_count, failure_count)."""
    info("Uploading to GHCR")
    uploaded = 0
    failures = 0
    for r in receipts:
        info(f"  Uploading NAR for {r['hash']} ({fmt_size(r['nar_size'])})")
        try:
            r["nar_digest"] = client.push_blob(r["nar_file"])
            uploaded += 1
        except (RuntimeError, OSError) as e:
            err(f"Failed to upload NAR for {r['hash']}: {e}")
            failures += 1

    if failures:
        err(
            f"{failures} upload(s) failed."
            f" Updating index with {uploaded} successful upload(s) only.",
        )
        receipts = [r for r in receipts if "nar_digest" in r]

    return receipts, uploaded, failures


def _build_entries(receipts: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    """Build index entries from upload receipts."""
    new_entries: dict[str, dict[str, Any]] = {}
    for r in receipts:
        store_path = ""
        for line in r["narinfo_text"].splitlines():
            if line.startswith("StorePath: "):
                store_path = line[len("StorePath: ") :].strip()
                break
        name = Path(store_path).name.split("-", 1)[-1] if store_path else r["hash"]
        new_entries[r["hash"]] = {
            "name": name,
            "narinfo": r["narinfo_text"],
            "nar_digest": r["nar_digest"],
            "nar_size": r["nar_size"],
            "added": utc_now(),
        }
    return new_entries


def _read_public_key(signing_key: str) -> str:
    """Read the public key file alongside the signing key."""
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
    args = parser.parse_args()

    client = OCIClient(push=True)
    signing_key = os.environ.get("NIXCACHE_SIGNING_KEY_FILE", "")

    upload_list = find_locally_built_paths(client)
    if not upload_list:
        info("Nothing to upload; store has no locally-built paths")
        return 0

    info(f"Found {len(upload_list)} locally-built paths to export")

    work_dir = tempfile.mkdtemp(prefix="nixcache-")
    cache_dir = Path(work_dir) / "cache"
    cache_dir.mkdir(parents=True, exist_ok=True)

    sign_paths(upload_list, signing_key)

    receipts = _export_all(upload_list, cache_dir)
    if not receipts:
        info("No paths exported successfully")
        return 0

    sample = random.choice(receipts)
    try:
        nar_self_check(sample)
    except (RuntimeError, OSError) as e:
        err(f"ABORTING: {e}")
        return 1

    public_key = _read_public_key(signing_key)

    receipts, uploaded, _ = _upload_nars(client, receipts)
    if uploaded == 0:
        info("No new paths uploaded")
        return 0

    gc_roots = _resolve_gc_root(args.out_link)
    new_entries = _build_entries(receipts)

    info(f"Uploaded {uploaded} NAR(s), updating index")
    index = update_index(client, new_entries, gc_roots, public_key=public_key, work_dir=work_dir)
    info(
        f"Cache index pushed to GHCR"
        f" ({len(index['entries'])} total entries, {len(new_entries)} new)",
    )

    return 0


if __name__ == "__main__":
    sys.exit(main())
