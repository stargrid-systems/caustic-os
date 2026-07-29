# Copyright (c) 2026 Simon Berger
# SPDX-License-Identifier: AGPL-3.0-only
"""OCI registry client for GHCR-backed Nix cache."""

import base64
import hashlib
import http.client
import json
import os
import threading
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import IO

from nixcache.config import (
    MANIFEST_MEDIA_TYPE,
    REGISTRY,
    REPO,
    err,
    sha256_file,
)

__all__ = ["OCIClient", "fetch_url", "open_stream"]

_HTTP_SERVER_ERROR = 500
_HTTP_OK = 200
_TOKEN_TTL = 240


def _urlopen(req: urllib.request.Request, timeout: int) -> http.client.HTTPResponse:
    """Open a URL request. URLs are constructed only from trusted constants."""
    return urllib.request.urlopen(req, timeout=timeout)  # noqa: S310


def _request(
    url: str,
    data: bytes | IO[bytes] | None = None,
    method: str | None = None,
) -> urllib.request.Request:
    """Create a Request object, centralized for S310 audit."""
    return urllib.request.Request(url, data=data, method=method)  # noqa: S310


def fetch_url(
    url: str,
    headers: dict[str, str] | None = None,
    timeout: int = 60,
    retries: int = 2,
) -> bytes | None:
    """Fetch a URL with retries on transient errors."""
    for attempt in range(retries + 1):
        req = _request(url)
        if headers:
            for k, v in headers.items():
                req.add_header(k, v)
        try:
            with _urlopen(req, timeout) as resp:
                return resp.read()
        except urllib.error.HTTPError as e:
            if e.code >= _HTTP_SERVER_ERROR and attempt < retries:
                time.sleep(1 + attempt)
                continue
            return None
        except (urllib.error.URLError, TimeoutError):
            if attempt < retries:
                time.sleep(1 + attempt)
                continue
            return None
    return None


def open_stream(
    url: str,
    headers: dict[str, str] | None = None,
    timeout: int = 120,
    retries: int = 2,
) -> tuple[http.client.HTTPResponse | None, int]:
    """Open a streaming connection to a URL with retries."""
    for attempt in range(retries + 1):
        req = _request(url)
        if headers:
            for k, v in headers.items():
                req.add_header(k, v)
        try:
            resp = _urlopen(req, timeout)
            length = resp.headers.get("Content-Length")
            return resp, int(length) if length else 0
        except urllib.error.HTTPError as e:
            if e.code >= _HTTP_SERVER_ERROR and attempt < retries:
                time.sleep(1 + attempt)
                continue
            return None, 0
        except (urllib.error.URLError, TimeoutError):
            if attempt < retries:
                time.sleep(1 + attempt)
                continue
            return None, 0
    return None, 0


class OCIClient:
    """Client for OCI registry operations (blob and manifest CRUD)."""

    def __init__(
        self,
        repo: str | None = None,
        registry: str | None = None,
        token: str | None = None,
        *,
        push: bool = False,
    ) -> None:
        """Initialize the OCI client with repo, registry, and optional token."""
        self.repo = repo if repo is not None else REPO
        self.registry = registry if registry is not None else REGISTRY
        self.push = push
        self._gh_token = token or os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN", "")
        self._oci_token = ""
        self._oci_token_time = 0.0
        self._token_lock = threading.Lock()
        if not self.repo:
            msg = "NIXCACHE_REPO is not set"
            raise RuntimeError(msg)

    @property
    def base(self) -> str:
        """Return the base OCI URL for the nix-cache repository."""
        return f"https://{self.registry}/v2/{self.repo}/nix-cache"

    def get_token(self) -> str:
        """Obtain or refresh the OCI authentication token."""
        with self._token_lock:
            if self._oci_token and (time.time() - self._oci_token_time) < _TOKEN_TTL:
                return self._oci_token

            scope_action = "pull,push" if self.push else "pull"
            scope = f"repository:{self.repo}/nix-cache:{scope_action}"
            token_url = f"https://{self.registry}/token?scope={scope}&service={self.registry}"

            if self._gh_token:
                creds = base64.b64encode(f"token:{self._gh_token}".encode()).decode()
                req = _request(token_url)
                req.add_header("Authorization", f"Basic {creds}")
                try:
                    with _urlopen(req, 10) as resp:
                        data = json.loads(resp.read())
                        self._oci_token = data.get("token", self._gh_token)
                        self._oci_token_time = time.time()
                        return self._oci_token
                except (urllib.error.URLError, TimeoutError, json.JSONDecodeError, OSError):
                    self._oci_token = self._gh_token
                    self._oci_token_time = time.time()
                    return self._oci_token

            data = fetch_url(token_url)
            if data:
                try:
                    self._oci_token = json.loads(data).get("token", "")
                    self._oci_token_time = time.time()
                except json.JSONDecodeError:
                    pass
                else:
                    return self._oci_token
            return ""

    def _auth_headers(self) -> dict[str, str]:
        token = self.get_token()
        h: dict[str, str] = {}
        if token:
            h["Authorization"] = f"Bearer {token}"
        return h

    def get_manifest(self, tag: str) -> tuple[bytes | None, str | None]:
        """Fetch a manifest by tag, returning (body, digest) or (None, None)."""
        req = _request(f"{self.base}/manifests/{tag}")
        for k, v in self._auth_headers().items():
            req.add_header(k, v)
        req.add_header("Accept", MANIFEST_MEDIA_TYPE)
        try:
            with _urlopen(req, 60) as resp:
                body = resp.read()
                return body, "sha256:" + hashlib.sha256(body).hexdigest()
        except (urllib.error.HTTPError, urllib.error.URLError, TimeoutError):
            return None, None

    def get_blob(self, digest: str, timeout: int = 120) -> bytes | None:
        """Fetch a blob by digest."""
        return fetch_url(f"{self.base}/blobs/{digest}", self._auth_headers(), timeout=timeout)

    def stream_blob(self, digest: str) -> tuple[http.client.HTTPResponse | None, int]:
        """Open a streaming connection to a blob."""
        return open_stream(f"{self.base}/blobs/{digest}", self._auth_headers())

    def push_blob(self, file_path: str | Path) -> str:
        """Upload a file as a blob, returning its digest."""
        digest = "sha256:" + sha256_file(file_path)

        if self._blob_exists(digest):
            return digest

        path = Path(file_path)
        size = path.stat().st_size

        upload_url = self._init_upload()
        if not upload_url:
            msg = "Failed to initiate blob upload"
            raise RuntimeError(msg)
        if upload_url.startswith("/"):
            upload_url = f"https://{self.registry}{upload_url}"

        sep = "&" if "?" in upload_url else "?"
        put_url = f"{upload_url}{sep}digest={digest}"

        with path.open("rb") as f:
            req = _request(put_url, data=f, method="PUT")
            for k, v in self._auth_headers().items():
                req.add_header(k, v)
            req.add_header("Content-Type", "application/octet-stream")
            req.add_header("Content-Length", str(size))
            try:
                _urlopen(req, 300)
            except urllib.error.HTTPError as e:
                msg = f"Blob upload failed (HTTP {e.code})"
                raise RuntimeError(msg) from e
            except (urllib.error.URLError, TimeoutError) as e:
                msg = f"Blob upload failed: {e}"
                raise RuntimeError(msg) from e

        return digest

    def _blob_exists(self, digest: str) -> bool:
        req = _request(f"{self.base}/blobs/{digest}", method="HEAD")
        for k, v in self._auth_headers().items():
            req.add_header(k, v)
        try:
            with _urlopen(req, 30) as resp:
                return resp.status == _HTTP_OK
        except (urllib.error.HTTPError, urllib.error.URLError, TimeoutError):
            return False

    def _init_upload(self) -> str:
        req = _request(f"{self.base}/blobs/uploads/", method="POST")
        for k, v in self._auth_headers().items():
            req.add_header(k, v)
        try:
            with _urlopen(req, 30) as resp:
                return resp.headers.get("Location", "")
        except urllib.error.HTTPError as e:
            err(f"Upload init failed (HTTP {e.code})")
            return ""
        except (urllib.error.URLError, TimeoutError):
            return ""

    def push_manifest(
        self,
        tag: str,
        manifest_json: str | bytes,
        if_match: str | None = None,
    ) -> tuple[bool, int]:
        """Push a manifest, returning (success, status_code)."""
        url = f"{self.base}/manifests/{tag}"
        data = manifest_json.encode() if isinstance(manifest_json, str) else manifest_json
        req = _request(url, data=data, method="PUT")
        for k, v in self._auth_headers().items():
            req.add_header(k, v)
        req.add_header("Content-Type", MANIFEST_MEDIA_TYPE)
        if if_match:
            req.add_header("If-Match", if_match)
        try:
            with _urlopen(req, 60) as resp:
                return True, resp.status
        except urllib.error.HTTPError as e:
            return False, e.code
        except (urllib.error.URLError, TimeoutError):
            return False, 0
