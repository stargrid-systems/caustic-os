#!/usr/bin/env python3
"""Prune stale entries from the OCI cache index."""

import argparse
import os
import sys
import tempfile
from datetime import datetime, timedelta, timezone

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import nixcache
from nixcache import OCIClient, fetch_index, info, push_index


def main():
    parser = argparse.ArgumentParser(
        description="Prune stale entries from the OCI cache index",
    )
    parser.add_argument(
        "live_file",
        help="File containing live store hashes (one per line)",
    )
    parser.add_argument("--retention-days", type=int, default=30)
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()

    client = OCIClient(push=True)

    existing, _ = fetch_index(client)
    if not existing:
        info("No cache index found, nothing to GC")
        return 0

    with open(args.live_file) as f:
        live = set(f.read().strip().split("\n"))

    cutoff = (
        datetime.now(timezone.utc) - timedelta(days=args.retention_days)
    ).strftime("%Y-%m-%dT%H:%M:%SZ")
    info(f"Retention: {args.retention_days} days (cutoff: {cutoff})")
    info(f"Dry run: {args.dry_run}")

    keep = {}
    delete = []
    for h, entry in existing.get("entries", {}).items():
        added = entry.get("added", "2099-01-01T00:00:00Z")
        if h in live or added >= cutoff:
            keep[h] = entry
        else:
            delete.append((h, entry))
            info(f"DELETE: {h} ({entry.get('name', '?')}) added={added}")

    info(f"Total: {len(keep)} keep, {len(delete)} delete")

    if args.dry_run or not delete:
        return 0

    existing["entries"] = keep
    existing["gc_roots"] = [h for h in existing.get("gc_roots", []) if h in keep]

    work_dir = tempfile.mkdtemp(prefix="nixcache-gc-")
    push_index(client, existing, work_dir)
    info("GC complete, index updated")
    return 0


if __name__ == "__main__":
    sys.exit(main())
