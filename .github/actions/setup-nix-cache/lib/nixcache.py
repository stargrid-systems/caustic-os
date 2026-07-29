"""Shared library for the nixcache-oci cache.

Provides the OCI registry client, cache-index data model, and NAR export
logic used by both the local proxy (proxy/main.py) and the upload/GC
CLIs (lib/upload.py, lib/gc.py).

Stdlib only, no pip dependencies.
"""

import base64
import hashlib
import json
import os
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone

# ── Config ────────────────────────────────────────────────────────────

REGISTRY = os.environ.get("NIXCACHE_REGISTRY", "ghcr.io")
REPO = os.environ.get("NIXCACHE_REPO", "")
IMAGE = f"{REGISTRY}/{REPO}/nix-cache" if REPO else ""

UPSTREAM_CACHES = os.environ.get(
    "NIXCACHE_UPSTREAM",
    os.environ.get("NIXCACHE_UPSTREAM_CACHES", "https://cache.nixos.org"),
).split()

INDEX_MEDIA_TYPE = "application/vnd.nix.cache.index.v1+json"
MANIFEST_MEDIA_TYPE = "application/vnd.oci.image.manifest.v1+json"
CONFIG_MEDIA_TYPE = "application/vnd.oci.image.config.v1+json"
STREAM_CHUNK_SIZE = 64 * 1024


# ── Logging helpers ───────────────────────────────────────────────────

def info(msg):
    print(f">>> {msg}", file=sys.stderr)


def err(msg):
    print(f"!!! {msg}", file=sys.stderr)


# ── Utility ───────────────────────────────────────────────────────────

def utc_now():
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def fmt_size(n):
    for unit in ("B", "KiB", "MiB", "GiB"):
        if n < 1024:
            return f"{n:.0f}{unit}" if unit == "B" else f"{n:.1f}{unit}"
        n /= 1024
    return f"{n:.1f}TiB"


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def store_hash(store_path):
    """Extract the 32-char hash prefix from a store path."""
    return os.path.basename(store_path)[:32]


# ── HTTP helpers ──────────────────────────────────────────────────────

def fetch_url(url, headers=None, timeout=60):
    """Fetch a URL fully into memory. Returns None on HTTP/network errors."""
    req = urllib.request.Request(url)
    if headers:
        for k, v in headers.items():
            req.add_header(k, v)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return resp.read()
    except (urllib.error.HTTPError, urllib.error.URLError, TimeoutError):
        return None


def open_stream(url, headers=None, timeout=120):
    """Open a streaming connection. Returns (response, content_length) or (None, 0)."""
    req = urllib.request.Request(url)
    if headers:
        for k, v in headers.items():
            req.add_header(k, v)
    try:
        resp = urllib.request.urlopen(req, timeout=timeout)
        length = resp.headers.get("Content-Length")
        return resp, int(length) if length else None
    except (urllib.error.HTTPError, urllib.error.URLError, TimeoutError):
        return None, 0


# ── OCI registry client ───────────────────────────────────────────────

class OCIClient:
    """HTTP client for a single OCI registry repository (GHCR)."""

    def __init__(self, repo=None, registry=None, token=None, push=False):
        self.repo = repo if repo is not None else REPO
        self.registry = registry if registry is not None else REGISTRY
        self.push = push
        self._gh_token = token or os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN", "")
        self._oci_token = ""
        self._oci_token_time = 0.0
        self._token_lock = threading.Lock()
        if not self.repo:
            raise RuntimeError("NIXCACHE_REPO is not set")

    @property
    def base(self):
        return f"https://{self.registry}/v2/{self.repo}/nix-cache"

    def get_token(self):
        with self._token_lock:
            if self._oci_token and (time.time() - self._oci_token_time) < 240:
                return self._oci_token

            scope_action = "pull,push" if self.push else "pull"
            scope = f"repository:{self.repo}/nix-cache:{scope_action}"
            token_url = f"https://{self.registry}/token?scope={scope}&service={self.registry}"

            if self._gh_token:
                creds = base64.b64encode(f"token:{self._gh_token}".encode()).decode()
                req = urllib.request.Request(token_url)
                req.add_header("Authorization", f"Basic {creds}")
                try:
                    with urllib.request.urlopen(req, timeout=10) as resp:
                        data = json.loads(resp.read())
                        self._oci_token = data.get("token", self._gh_token)
                        self._oci_token_time = time.time()
                        return self._oci_token
                except Exception:
                    self._oci_token = self._gh_token
                    self._oci_token_time = time.time()
                    return self._oci_token

            data = fetch_url(token_url)
            if data:
                try:
                    self._oci_token = json.loads(data).get("token", "")
                    self._oci_token_time = time.time()
                    return self._oci_token
                except json.JSONDecodeError:
                    pass
            return ""

    def _auth_headers(self):
        token = self.get_token()
        h = {}
        if token:
            h["Authorization"] = f"Bearer {token}"
        return h

    def get_manifest(self, tag):
        """Fetch a manifest by tag. Returns (body_bytes, digest) or (None, None)."""
        req = urllib.request.Request(f"{self.base}/manifests/{tag}")
        for k, v in self._auth_headers().items():
            req.add_header(k, v)
        req.add_header("Accept", MANIFEST_MEDIA_TYPE)
        try:
            with urllib.request.urlopen(req, timeout=60) as resp:
                body = resp.read()
                return body, "sha256:" + hashlib.sha256(body).hexdigest()
        except (urllib.error.HTTPError, urllib.error.URLError, TimeoutError):
            return None, None

    def get_blob(self, digest, timeout=120):
        return fetch_url(f"{self.base}/blobs/{digest}", self._auth_headers(), timeout=timeout)

    def stream_blob(self, digest):
        return open_stream(f"{self.base}/blobs/{digest}", self._auth_headers())

    def push_blob(self, file_path):
        """Push a file as a content-addressed OCI blob. Returns the digest."""
        digest = "sha256:" + sha256_file(file_path)

        if self._blob_exists(digest):
            return digest

        size = os.path.getsize(file_path)

        upload_url = self._init_upload()
        if not upload_url:
            raise RuntimeError("Failed to initiate blob upload")
        if upload_url.startswith("/"):
            upload_url = f"https://{self.registry}{upload_url}"

        sep = "&" if "?" in upload_url else "?"
        put_url = f"{upload_url}{sep}digest={digest}"

        with open(file_path, "rb") as f:
            req = urllib.request.Request(put_url, data=f, method="PUT")
            for k, v in self._auth_headers().items():
                req.add_header(k, v)
            req.add_header("Content-Type", "application/octet-stream")
            req.add_header("Content-Length", str(size))
            try:
                urllib.request.urlopen(req, timeout=300)
            except urllib.error.HTTPError as e:
                raise RuntimeError(f"Blob upload failed (HTTP {e.code})")
            except (urllib.error.URLError, TimeoutError) as e:
                raise RuntimeError(f"Blob upload failed: {e}")

        return digest

    def _blob_exists(self, digest):
        req = urllib.request.Request(f"{self.base}/blobs/{digest}", method="HEAD")
        for k, v in self._auth_headers().items():
            req.add_header(k, v)
        try:
            with urllib.request.urlopen(req, timeout=30) as resp:
                return resp.status == 200
        except (urllib.error.HTTPError, urllib.error.URLError, TimeoutError):
            return False

    def _init_upload(self):
        req = urllib.request.Request(f"{self.base}/blobs/uploads/", method="POST")
        for k, v in self._auth_headers().items():
            req.add_header(k, v)
        try:
            with urllib.request.urlopen(req, timeout=30) as resp:
                return resp.headers.get("Location", "")
        except urllib.error.HTTPError as e:
            err(f"Upload init failed (HTTP {e.code})")
            return ""
        except (urllib.error.URLError, TimeoutError):
            return ""

    def push_manifest(self, tag, manifest_json, if_match=None):
        """Push a manifest and tag it. Returns (success, http_code)."""
        url = f"{self.base}/manifests/{tag}"
        data = manifest_json.encode() if isinstance(manifest_json, str) else manifest_json
        req = urllib.request.Request(url, data=data, method="PUT")
        for k, v in self._auth_headers().items():
            req.add_header(k, v)
        req.add_header("Content-Type", MANIFEST_MEDIA_TYPE)
        if if_match:
            req.add_header("If-Match", if_match)
        try:
            with urllib.request.urlopen(req, timeout=60) as resp:
                return True, resp.status
        except urllib.error.HTTPError as e:
            return False, e.code
        except (urllib.error.URLError, TimeoutError):
            return False, 0


# ── Cache index ───────────────────────────────────────────────────────

def fetch_index(client):
    """Fetch and parse the cache-index from the registry. Returns dict or None."""
    manifest_data, _ = client.get_manifest("cache-index")
    if manifest_data is None:
        return None
    try:
        manifest = json.loads(manifest_data)
        layers = manifest.get("layers", [])
        if not layers:
            return None
        index_digest = layers[0]["digest"]
        index_data = client.get_blob(index_digest)
        if index_data is None:
            return None
        return json.loads(index_data)
    except (json.JSONDecodeError, KeyError):
        return None


def merge_index(existing, new_entries, gc_roots, public_key=""):
    """Build a merged cache-index dict from existing entries and new uploads."""
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
    """Build the OCI manifest JSON for the cache-index tag."""
    return json.dumps({
        "schemaVersion": 2,
        "mediaType": MANIFEST_MEDIA_TYPE,
        "config": {
            "mediaType": CONFIG_MEDIA_TYPE,
            "digest": config_digest,
            "size": config_size,
        },
        "layers": [{
            "mediaType": INDEX_MEDIA_TYPE,
            "digest": index_digest,
            "size": index_size,
        }],
    })


def push_index(client, index, work_dir, if_match=None):
    """Push the cache-index (blob + config + manifest) to the registry."""
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
        raise RuntimeError(f"Failed to push cache-index manifest (HTTP {code})")


def update_index(client, new_entries, gc_roots, public_key="", work_dir=None):
    """Fetch existing index, merge, and push. Returns the merged index dict."""
    if work_dir is None:
        import tempfile
        work_dir = tempfile.mkdtemp(prefix="nixcache-")

    existing = fetch_index(client)
    index = merge_index(existing, new_entries, gc_roots, public_key)
    push_index(client, index, work_dir)
    return index


# ── NAR export ────────────────────────────────────────────────────────

def sign_paths(paths, key_file):
    """Sign store paths with the given signing key. Non-fatal on failure."""
    if not key_file or not paths:
        return
    info(f"Signing {len(paths)} store paths")
    subprocess.run(
        ["nix", "store", "sign", "--key-file", key_file] + paths,
        check=False,
    )


def generate_narinfo(store_path, hash_prefix, file_size, file_hash, path_info_json):
    """Generate narinfo text for a store path."""
    info = json.loads(path_info_json)
    if isinstance(info, list):
        info = info[0]
    elif isinstance(info, dict) and store_path in info:
        info = info[store_path]

    nar_hash = info.get("narHash", "")
    nar_size = info.get("narSize", 0)
    refs = info.get("references", [])
    deriver = info.get("deriver", "")
    sigs = info.get("signatures", info.get("sigs", []))

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
    """Export a single store path to a compressed NAR and generate its narinfo.

    Returns a dict with hash, nar_file, narinfo_text, nar_size.
    """
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
        capture_output=True, text=True, check=True,
    ).stdout.strip()

    path_info_json = subprocess.run(
        ["nix", "path-info", "--json", store_path],
        capture_output=True, text=True, check=True,
    ).stdout

    narinfo_text = generate_narinfo(store_path, h, file_size, file_hash, path_info_json)

    return {
        "hash": h,
        "nar_file": nar_file,
        "narinfo_text": narinfo_text,
        "nar_size": file_size,
    }


# ── Path discovery ────────────────────────────────────────────────────

def find_locally_built_paths(client):
    """Return store paths built locally (unsigned) and not already in the index."""
    existing = fetch_index(client)
    own_hashes = set()
    if existing:
        own_hashes = set(existing.get("entries", {}).keys())
    info(f"GHCR index contains {len(own_hashes)} previously-cached entries")

    result = subprocess.run(
        ["nix", "path-info", "--all", "--json"],
        capture_output=True, text=True, check=True,
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
