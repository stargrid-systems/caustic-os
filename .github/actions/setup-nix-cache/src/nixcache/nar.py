import hashlib
import json
import lzma
import os
import subprocess

from nixcache.config import info, store_hash
from nixcache.index import fetch_index


def sign_paths(paths, key_file):
    if not key_file or not paths:
        return
    info(f"Signing {len(paths)} store paths")
    subprocess.run(
        ["nix", "store", "sign", "--key-file", key_file, *paths],
        check=False,
    )


def generate_narinfo(store_path, hash_prefix, file_size, file_hash, path_info_json):
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

    ref_names = " ".join(os.path.basename(r) for r in refs)

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
        lines.append(f"Deriver: {os.path.basename(deriver)}")
    for sig in sigs:
        lines.append(f"Sig: {sig}")

    return "\n".join(lines) + "\n"


def export_path(store_path, cache_dir):
    h = store_hash(store_path)
    nar_dir = os.path.join(cache_dir, "nar")
    os.makedirs(nar_dir, exist_ok=True)
    nar_file = os.path.join(nar_dir, f"{h}.nar.xz")

    dump = subprocess.Popen(
        ["nix-store", "--dump", store_path],
        stdout=subprocess.PIPE,
    )
    assert dump.stdout is not None
    with open(nar_file, "wb") as out:
        xz = subprocess.Popen(["xz", "-1"], stdin=dump.stdout, stdout=out)
        dump.stdout.close()
        xz.communicate()
        dump.wait()
        if dump.returncode != 0:
            raise RuntimeError(f"nix-store --dump failed for {store_path}")
        if xz.returncode != 0:
            raise RuntimeError(f"xz failed for {store_path}")

    file_size = os.path.getsize(nar_file)
    file_hash = subprocess.run(
        ["nix", "hash", "file", "--type", "sha256", "--base32", nar_file],
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
        "nar_file": nar_file,
        "narinfo_text": narinfo_text,
        "nar_size": file_size,
    }


def nar_self_check(receipt):
    store_path = receipt["store_path"]
    nar_file = receipt["nar_file"]
    h = receipt["hash"]

    dump = subprocess.Popen(
        ["nix-store", "--dump", store_path],
        stdout=subprocess.PIPE,
    )
    assert dump.stdout is not None
    actual_hasher = hashlib.sha256()
    while True:
        chunk = dump.stdout.read(65536)
        if not chunk:
            break
        actual_hasher.update(chunk)
    dump.stdout.close()
    dump.wait()
    if dump.returncode != 0:
        raise RuntimeError(f"self-check: nix-store --dump failed for {store_path}")

    stored_hasher = hashlib.sha256()
    with lzma.open(nar_file) as f:
        for chunk in iter(lambda: f.read(65536), b""):
            stored_hasher.update(chunk)

    actual = actual_hasher.hexdigest()
    stored = stored_hasher.hexdigest()
    if actual != stored:
        raise RuntimeError(
            f"NAR self-check failed for {h}: re-dump sha256 {actual} != stored sha256 {stored}"
        )
    info(f"NAR self-check passed for {h}")


def find_locally_built_paths(client):
    existing, _ = fetch_index(client)
    own_hashes = set()
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
