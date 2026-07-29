# Copyright (c) 2026 Simon Berger
# SPDX-License-Identifier: AGPL-3.0-only
"""Prune stale entries from the OCI cache index."""

import argparse
import sys
import tempfile
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import Any

from nixcache.config import info
from nixcache.index import fetch_index, push_index
from nixcache.oci import OCIClient

_FAR_FUTURE = "2099-01-01T00:00:00Z"


def _is_unsigned(entry: dict[str, Any]) -> bool:
    """Return True if the entry's narinfo has no Sig: line."""
    narinfo = entry.get("narinfo", "")
    return not any(line.strip().startswith("Sig:") for line in narinfo.splitlines())


def main() -> int:
    """Entry point for nixcache-gc."""
    parser = argparse.ArgumentParser(
        description="Prune stale entries from the OCI cache index",
    )
    parser.add_argument(
        "live_file",
        help="File containing live store hashes (one per line)",
    )
    parser.add_argument("--retention-days", type=int, default=30)
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument(
        "--purge-unsigned",
        action="store_true",
        help="Remove entries whose narinfo has no Sig: line",
    )
    args = parser.parse_args()

    client = OCIClient(push=True)

    existing, old_digest = fetch_index(client)
    if not existing:
        info("No cache index found, nothing to GC")
        return 0

    live = set(Path(args.live_file).read_text().strip().split("\n"))

    cutoff = (datetime.now(UTC) - timedelta(days=args.retention_days)).strftime(
        "%Y-%m-%dT%H:%M:%SZ",
    )
    info(f"Retention: {args.retention_days} days (cutoff: {cutoff})")
    info(f"Dry run: {args.dry_run}")
    info(f"Purge unsigned: {args.purge_unsigned}")

    keep: dict[str, Any] = {}
    delete: list[tuple[str, dict[str, Any]]] = []
    for h, entry in existing.get("entries", {}).items():
        if args.purge_unsigned and _is_unsigned(entry):
            delete.append((h, entry))
            info(f"DELETE (unsigned): {h} ({entry.get('name', '?')})")
            continue
        added = entry.get("added", _FAR_FUTURE)
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
    push_index(client, existing, work_dir, if_match=old_digest)
    info("GC complete, index updated")
    return 0


if __name__ == "__main__":
    sys.exit(main())
