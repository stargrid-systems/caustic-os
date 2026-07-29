# Copyright (c) 2026 Simon Berger
# SPDX-License-Identifier: AGPL-3.0-only
"""NAR export, narinfo generation, and self-check."""

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


def generate_narinfo(
    store_path: str,
    hash_prefix: str,
    file_size: int,
    file_hash: str,
    path_info_json: str,
) -> str:
    """Generate a narinfo string from nix path-info JSON output."""
    data = json.loads(path_info_json)
    if isinstance(data, list):
        data = data[0]
    elif isinstance(data, dict) and store_path in data:
        data = data[store_path]

    nar_hash = data.get("narHash", "")
    nar_size = data.get("narSize", 0)
    refs = data.get("references", [])
    deriver = data.get("deriver", "")
    sigs = data.get("signatures", data.get("sigs", []))

    ref_names = " ".join(Path(r).name for r in refs)

    lines = [
        f"StorePath: {store_path}",
        f"URL: nar/{hash_prefix}.nar.xz",
        "Compression: xz",
        f"FileHash: sha256:{file_hash}",
        f"FileSize: {file_size}",
        f"NarHash: {nar_hash}",
        f"NarSize: {nar_size}",
    ]
    if ref_names:
        lines.append(f"References: {ref_names}")
    if deriver:
        lines.append(f"Deriver: {Path(deriver).name}")
    lines.extend(f"Sig: {sig}" for sig in sigs)

    return "\n".join(lines) + "\n"


def export_path(store_path: str, cache_dir: str | Path) -> dict[str, Any]:
    """Export a single store path as a compressed NAR, returning receipt metadata."""
    h = store_hash(store_path)
    nar_dir = Path(cache_dir) / "nar"
    nar_dir.mkdir(parents=True, exist_ok=True)
    nar_file = nar_dir / f"{h}.nar.xz"

    dump = subprocess.Popen(
        ["nix-store", "--dump", store_path],
        stdout=subprocess.PIPE,
    )
    if dump.stdout is None:
        msg = f"nix-store --dump produced no stdout for {store_path}"
        raise RuntimeError(msg)
    with nar_file.open("wb") as out:
        xz = subprocess.Popen(["xz", "-1"], stdin=dump.stdout, stdout=out)
        dump.stdout.close()
        xz.communicate()
        dump.wait()
        if dump.returncode != 0:
            msg = f"nix-store --dump failed for {store_path}"
            raise RuntimeError(msg)
        if xz.returncode != 0:
            msg = f"xz failed for {store_path}"
            raise RuntimeError(msg)

    file_size = nar_file.stat().st_size
    file_hash = subprocess.run(
        ["nix", "hash", "file", "--type", "sha256", "--base32", str(nar_file)],
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()

    path_info_json = subprocess.run(
        ["nix", "path-info", "--json", store_path],
        capture_output=True,
        text=True,
        check=True,
    ).stdout

    narinfo_text = generate_narinfo(store_path, h, file_size, file_hash, path_info_json)

    return {
        "hash": h,
        "store_path": store_path,
        "nar_file": str(nar_file),
        "narinfo_text": narinfo_text,
        "nar_size": file_size,
    }


def nar_self_check(receipt: dict[str, Any]) -> None:
    """Re-dump a store path and verify the NAR SHA-256 matches the stored file."""
    store_path = receipt["store_path"]
    nar_file = Path(receipt["nar_file"])
    h = receipt["hash"]

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

    actual = actual_hasher.hexdigest()
    stored = stored_hasher.hexdigest()
    if actual != stored:
        msg = f"NAR self-check failed for {h}: re-dump sha256 {actual} != stored sha256 {stored}"
        raise RuntimeError(msg)
    info(f"NAR self-check passed for {h}")


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
