import base64
import hashlib
import json
import os
import threading
import time
import urllib.error
import urllib.request

from nixcache.config import (
    MANIFEST_MEDIA_TYPE,
    REGISTRY,
    REPO,
    err,
    sha256_file,
)

__all__ = ["OCIClient", "fetch_url", "open_stream"]


def fetch_url(url, headers=None, timeout=60, retries=2):
    for attempt in range(retries + 1):
        req = urllib.request.Request(url)
        if headers:
            for k, v in headers.items():
                req.add_header(k, v)
        try:
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                return resp.read()
        except urllib.error.HTTPError as e:
            if e.code >= 500 and attempt < retries:
                time.sleep(1 + attempt)
                continue
            return None
        except (urllib.error.URLError, TimeoutError):
            if attempt < retries:
                time.sleep(1 + attempt)
                continue
            return None


def open_stream(url, headers=None, timeout=120, retries=2):
    for attempt in range(retries + 1):
        req = urllib.request.Request(url)
        if headers:
            for k, v in headers.items():
                req.add_header(k, v)
        try:
            resp = urllib.request.urlopen(req, timeout=timeout)
            length = resp.headers.get("Content-Length")
            return resp, int(length) if length else None
        except urllib.error.HTTPError as e:
            if e.code >= 500 and attempt < retries:
                time.sleep(1 + attempt)
                continue
            return None, 0
        except (urllib.error.URLError, TimeoutError):
            if attempt < retries:
                time.sleep(1 + attempt)
                continue
            return None, 0


class OCIClient:
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
                raise RuntimeError(f"Blob upload failed (HTTP {e.code})") from e
            except (urllib.error.URLError, TimeoutError) as e:
                raise RuntimeError(f"Blob upload failed: {e}") from e

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
