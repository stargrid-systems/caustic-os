# Copyright (c) 2026 Simon Berger
# SPDX-License-Identifier: AGPL-3.0-only
"""NAR export via nix copy and narinfo parsing."""

import hashlib
import json
import lzma
import subprocess
from pathlib import Path
from typing import Any

from nixcache.config import info, store_hash
from nixcache.index import fetch_index
from nixcache.oci import OCIClient

_NAR_CHUNK_SIZE = 65536


def sign_paths(paths: list[str], key_file: str) -> None:
    """Sign store paths with the given key using nix store sign."""
    if not key_file or not paths:
        return
    info(f"Signing {len(paths)} store paths")
    subprocess.run(
        ["nix", "store", "sign", "--key-file", key_file, *paths],
        check=False,
    )


def export_to_binary_cache(
    paths: list[str],
    cache_dir: Path,
    signing_key: str,
) -> None:
    """Export store paths to a local binary cache via nix copy."""
    sign_paths(paths, signing_key)
    cache_dir.mkdir(parents=True, exist_ok=True)
    info(f"Exporting {len(paths)} paths via nix copy")
    subprocess.run(
        ["nix", "copy", "--to", f"file://{cache_dir}", *paths],
        check=True,
    )


def parse_narinfo(narinfo_path: Path) -> dict[str, Any]:
    """Parse a .narinfo file into a metadata dict."""
    text = narinfo_path.read_text()
    fields: dict[str, Any] = {"text": text, "hash": narinfo_path.stem}
    for line in text.splitlines():
        if ": " not in line:
            continue
        key, value = line.split(": ", 1)
        lk = key.lower()
        if lk == "references":
            fields["references"] = value.split()
        elif lk == "sig":
            fields.setdefault("sigs", []).append(value)
        else:
            fields[lk] = value
    return fields


def scan_binary_cache(cache_dir: Path, wanted: set[str]) -> list[dict[str, Any]]:
    """Scan a binary cache for narinfo + NAR pairs matching wanted hashes."""
    entries: list[dict[str, Any]] = []
    for narinfo_path in sorted(cache_dir.glob("*.narinfo")):
        if narinfo_path.stem not in wanted:
            continue
        entry = parse_narinfo(narinfo_path)
        nar_path = cache_dir / entry.get("url", "")
        if not nar_path.exists():
            continue
        entry["nar_file"] = str(nar_path)
        entry["nar_size"] = nar_path.stat().st_size
        entries.append(entry)
    return entries


def nar_self_check(entry: dict[str, Any]) -> None:
    """Verify a NAR file matches the store path by re-dumping and comparing."""
    store_path = entry.get("storepath", "")
    nar_file = Path(entry["nar_file"])

    dump = subprocess.Popen(
        ["nix-store", "--dump", store_path],
        stdout=subprocess.PIPE,
    )
    if dump.stdout is None:
        msg = f"self-check: nix-store --dump produced no stdout for {store_path}"
        raise RuntimeError(msg)
    actual_hasher = hashlib.sha256()
    while True:
        chunk = dump.stdout.read(_NAR_CHUNK_SIZE)
        if not chunk:
            break
        actual_hasher.update(chunk)
    dump.stdout.close()
    dump.wait()
    if dump.returncode != 0:
        msg = f"self-check: nix-store --dump failed for {store_path}"
        raise RuntimeError(msg)

    stored_hasher = hashlib.sha256()
    with lzma.open(nar_file) as f:
        for chunk in iter(lambda: f.read(_NAR_CHUNK_SIZE), b""):
            stored_hasher.update(chunk)

    actual = f"sha256:{actual_hasher.hexdigest()}"
    stored = f"sha256:{stored_hasher.hexdigest()}"
    if actual != stored:
        msg = f"NAR self-check failed for {entry['hash']}: {actual} != {stored}"
        raise RuntimeError(msg)
    info(f"NAR self-check passed for {entry['hash']}")


def find_locally_built_paths(client: OCIClient) -> list[str]:
    """Return unsigned store paths not already present in the OCI cache index."""
    existing, _ = fetch_index(client)
    own_hashes: set[str] = set()
    if existing:
        own_hashes = set(existing.get("entries", {}).keys())
    info(f"GHCR index contains {len(own_hashes)} previously-cached entries")

    result = subprocess.run(
        ["nix", "path-info", "--all", "--json"],
        capture_output=True,
        text=True,
        check=True,
    )
    all_paths = json.loads(result.stdout)

    if isinstance(all_paths, list):
        items = all_paths
    else:
        items = [{"path": k, **v} for k, v in all_paths.items()]

    paths = []
    for item in items:
        sigs = item.get("signatures", item.get("sigs", []))
        if sigs:
            continue
        path = item.get("path", "")
        if not path:
            continue
        h = store_hash(path)
        if h in own_hashes:
            continue
        paths.append(path)

    return sorted(set(paths))
