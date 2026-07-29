#!/usr/bin/env python3
"""Upload locally-built Nix store paths to the OCI cache on GHCR."""

import argparse
import os
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import nixcache
from nixcache import (
    OCIClient,
    export_path,
    find_locally_built_paths,
    info,
    err,
    sign_paths,
    store_hash,
    update_index,
)


def main():
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
    cache_dir = os.path.join(work_dir, "cache")
    os.makedirs(cache_dir, exist_ok=True)

    sign_paths(upload_list, signing_key)

    info(f"Exporting {len(upload_list)} store paths (direct NAR dump, skipping full closure)")
    receipts = []
    for store_path in upload_list:
        try:
            result = export_path(store_path, cache_dir)
            info(f"  Exported {result['hash']} ({nixcache.fmt_size(result['nar_size'])})")
            receipts.append(result)
        except Exception as e:
            err(f"Failed to export {store_path}: {e}")

    if not receipts:
        info("No paths exported successfully")
        return 0

    public_key = ""
    if signing_key and os.path.exists(signing_key + ".pub"):
        with open(signing_key + ".pub") as f:
            public_key = f.read().strip()

    info(f"Uploading to GHCR: {nixcache.IMAGE}")
    uploaded = 0
    failures = 0
    for r in receipts:
        info(f"  Uploading NAR for {r['hash']} ({nixcache.fmt_size(r['nar_size'])})")
        try:
            r["nar_digest"] = client.push_blob(r["nar_file"])
            uploaded += 1
        except Exception as e:
            err(f"Failed to upload NAR for {r['hash']}: {e}")
            failures += 1

    if uploaded == 0:
        info("No new paths uploaded")
        return 0

    if failures:
        err(f"{failures} upload(s) failed. Updating index with {uploaded} successful upload(s) only.")
        receipts = [r for r in receipts if "nar_digest" in r]

    gc_roots = []
    try:
        result = subprocess.run(
            ["nix", "path-info", args.out_link],
            capture_output=True, text=True, check=True,
        )
        top_level = result.stdout.strip()
        if top_level:
            gc_roots = [store_hash(top_level)]
    except subprocess.CalledProcessError:
        pass

    info(f"Uploaded {uploaded} NAR(s), updating index")

    new_entries = {}
    for r in receipts:
        store_path = ""
        for line in r["narinfo_text"].splitlines():
            if line.startswith("StorePath: "):
                store_path = line[len("StorePath: "):].strip()
                break
        name = (
            os.path.basename(store_path).split("-", 1)[-1]
            if store_path else r["hash"]
        )
        new_entries[r["hash"]] = {
            "name": name,
            "narinfo": r["narinfo_text"],
            "nar_digest": r["nar_digest"],
            "nar_size": r["nar_size"],
            "added": nixcache.utc_now(),
        }

    index = update_index(client, new_entries, gc_roots, public_key, work_dir)
    info(
        f"Cache index pushed to GHCR "
        f"({len(index['entries'])} total entries, {len(new_entries)} new)"
    )

    return 0


if __name__ == "__main__":
    sys.exit(main())
