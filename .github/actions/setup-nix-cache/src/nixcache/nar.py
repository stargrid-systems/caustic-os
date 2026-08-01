# Copyright (c) 2026 Simon Berger
# SPDX-License-Identifier: AGPL-3.0-only
"""NAR dump, narinfo generation, and local-path discovery."""

import hashlib
import json
import lzma
import os
import subprocess
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path
from typing import Any

from nixcache.config import err, info, sha256_file, store_hash
from nixcache.index import fetch_index
from nixcache.oci import OCIClient

_NAR_CHUNK_SIZE = 65536
_DUMP_WORKERS = min(os.cpu_count() or 4, 8)


def sanitize_narinfo(text: str) -> str:
    """Fix known narinfo issues (bad Deriver field with full path)."""
    lines = text.split("\n")
    fixed: list[str] = []
    for line in lines:
        if line.startswith("Deriver: /nix/store/"):
            deriver = line.split(": ", 1)[1]
            fixed.append(f"Deriver: {Path(deriver).name}")
        else:
            fixed.append(line)
    return "\n".join(fixed)


def sign_paths(paths: list[str], key_file: str) -> None:
    """Sign store paths with the given key using nix store sign."""
    if not key_file or not paths:
        return
    info(f"Signing {len(paths)} store paths")
    subprocess.run(
        ["nix", "store", "sign", "--key-file", key_file, *paths],
        check=True,
    )


def _query_path_info(paths: list[str]) -> dict[str, dict[str, Any]]:
    """Return a store-path keyed dict of verbose path-info metadata."""
    result = subprocess.run(
        ["nix", "path-info", "--json", "-v", *paths],
        capture_output=True,
        text=True,
        check=True,
    )
    raw = json.loads(result.stdout)
    if isinstance(raw, list):
        return {item["path"]: item for item in raw}
    return {k: {"path": k, **v} for k, v in raw.items()}


def _dump_nar(store_path: str, dest: Path) -> str:
    """Dump a single store path to a compressed NAR file.

    Returns the sha256 hash of the compressed file.
    """
    dest.parent.mkdir(parents=True, exist_ok=True)
    with dest.open("wb") as f:
        dump = subprocess.Popen(
            ["nix-store", "--dump", store_path],
            stdout=subprocess.PIPE,
        )
        xz = subprocess.Popen(
            ["xz", "-6", "-e"],
            stdin=dump.stdout,
            stdout=f,
        )
        if dump.stdout is not None:
            dump.stdout.close()
        xz.wait()
        dump.wait()
    if xz.returncode != 0:
        msg = f"xz failed (exit {xz.returncode}) for {store_path}"
        raise RuntimeError(msg)
    if dump.returncode != 0:
        msg = f"nix-store --dump failed (exit {dump.returncode}) for {store_path}"
        raise RuntimeError(msg)
    return "sha256:" + sha256_file(dest)


def _build_narinfo(
    meta: dict[str, Any],
    nar_url: str,
    file_hash: str,
    file_size: int,
) -> str:
    """Build a .narinfo text block from verbose path-info metadata."""
    refs = " ".join(Path(r).name for r in meta.get("references", []))
    lines = [
        f"StorePath: {meta['path']}",
        f"URL: {nar_url}",
        "Compression: xz",
        f"FileHash: {file_hash}",
        f"FileSize: {file_size}",
        f"NarHash: {meta['narHash']}",
        f"NarSize: {meta['narSize']}",
    ]
    if refs:
        lines.append(f"References: {refs}")
    if meta.get("deriver") and meta["deriver"] != "unknown":
        lines.append(f"Deriver: {Path(meta['deriver']).name}")
    lines.extend(f"Sig: {sig}" for sig in meta.get("signatures", []))
    if meta.get("system"):
        lines.append(f"System: {meta['system']}")
    return "\n".join(lines) + "\n"


def dump_nars(
    paths: list[str],
    cache_dir: Path,
    signing_key: str,
) -> list[dict[str, Any]]:
    """Sign, dump, and build narinfo for each path individually.

    Unlike nix copy --to, this only processes the given paths
    and does NOT export their dependency closure. Dumping is
    CPU-bound (xz), so paths are processed in parallel.
    """
    sign_paths(paths, signing_key)
    cache_dir.mkdir(parents=True, exist_ok=True)
    info_map = _query_path_info(paths)
    info(f"Dumping {len(paths)} NARs in parallel ({_DUMP_WORKERS} workers)")

    def dump_one(store_path: str) -> dict[str, Any] | None:
        """Dump a single store path to a NAR and build its narinfo."""
        meta = info_map.get(store_path)
        if meta is None:
            return None
        h = store_hash(store_path)
        nar_url = f"nar/{h}.nar.xz"
        nar_file = cache_dir / f"{h}.nar.xz"
        file_hash = _dump_nar(store_path, nar_file)
        file_size = nar_file.stat().st_size
        narinfo_text = _build_narinfo(meta, nar_url, file_hash, file_size)
        return {
            "hash": h,
            "text": narinfo_text,
            "nar_file": str(nar_file),
            "nar_size": file_size,
            "storepath": store_path,
            "url": nar_url,
        }

    entries: list[dict[str, Any]] = []
    failures = 0
    with ThreadPoolExecutor(max_workers=_DUMP_WORKERS) as pool:
        futures = {pool.submit(dump_one, p): p for p in paths}
        for future in as_completed(futures):
            path = futures[future]
            try:
                result = future.result()
            except (RuntimeError, OSError) as e:
                err(f"Failed to dump NAR for {path}: {e}")
                failures += 1
                continue
            if result is not None:
                entries.append(result)

    if failures:
        err(f"{failures} dump(s) failed, continuing with {len(entries)} successful")

    info(f"Dumped {len(entries)} NARs")
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


def _is_signed_narinfo(existing: dict[str, Any] | None, h: str) -> bool:
    """Check if a cached index entry has at least one Sig: line."""
    if not existing:
        return False
    entry = existing.get("entries", {}).get(h)
    if entry is None:
        return False
    return "Sig:" in entry.get("narinfo", "")


def find_locally_built_paths(client: OCIClient, force: bool = False) -> list[str]:
    """Return store paths that need uploading.

    Re-includes cached entries whose narinfo lacks signatures so they
    get re-signed on the next build.

    When force is True, skips the index check and returns all non-baseline,
    non-drv paths. Use this to repair missing NAR blobs.
    """
    existing, _ = fetch_index(client) if not force else (None, None)
    own_hashes: set[str] = set()
    if existing:
        own_hashes = set(existing.get("entries", {}).keys())
    info(f"GHCR index contains {len(own_hashes)} previously-cached entries")

    baseline: set[str] = set()
    baseline_file = Path(os.environ.get("NIXCACHE_STORE_BASELINE", ""))
    if baseline_file.exists():
        baseline = set(baseline_file.read_text().split())
        info(f"Store baseline: {len(baseline)} paths (will skip these)")

    result = subprocess.run(
        ["nix", "path-info", "--all", "--json", "-v"],
        capture_output=True,
        text=True,
        check=True,
    )
    all_paths = json.loads(result.stdout)

    if isinstance(all_paths, list):
        items = all_paths
    else:
        items = [{"path": k, **v} for k, v in all_paths.items()]

    unsigned = 0
    signed = 0
    skipped_drv = 0
    reupload_unsigned = 0
    paths = []
    for item in items:
        path = item.get("path", "")
        if not path or path in baseline:
            continue
        if path.endswith(".drv"):
            skipped_drv += 1
            continue
        sigs = item.get("signatures", item.get("sigs", []))
        if sigs:
            signed += 1
            if not force:
                continue
        unsigned += 1
        if not force:
            h = store_hash(path)
            if h in own_hashes:
                if _is_signed_narinfo(existing, h):
                    continue
                reupload_unsigned += 1
        paths.append(path)

    info(
        f"Store scan: {signed} signed (cached), {unsigned} unsigned"
        f" ({reupload_unsigned} re-upload to fix missing signatures),"
        f" {skipped_drv} .drv skipped",
    )
    return sorted(set(paths))
