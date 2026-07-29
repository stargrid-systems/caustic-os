# nixcache-oci

Nix binary cache backed by GHCR (OCI registry). Originally forked from
[cmspam/nixcache-oci](https://github.com/cmspam/nixcache-oci), now a
first-party component with significant divergence.

## How it works

NARs are stored as content-addressed OCI blobs under
`ghcr.io/<repo>/nix-cache`. A single `cache-index` tag holds a JSON
manifest mapping every store hash to its narinfo text and NAR blob digest.

- **Proxy** (`proxy/main.py`): local HTTP server that bridges the Nix
  binary cache protocol to GHCR. Serves narinfo from an in-memory index
  (zero network latency), streams NAR blobs directly from GHCR.
- **Uploader** (`lib/upload.py`): exports locally-built store paths,
  pushes NARs as OCI blobs, merges entries into the index.
- **GC** (`lib/gc.py`): prunes index entries older than the retention
  period that are not in the current live closure.
- **Shared library** (`lib/nixcache.py`): OCI registry client, index
  model, and NAR export logic used by all three.

Stdlib only (Python 3, no pip dependencies). Also requires `curl`, `jq`,
`xz`, and the Nix CLI tools.

## Actions

- `setup-nix-cache` - starts the proxy and configures Nix to use it as
  a substituter.
- `setup-nix-cache/save` - uploads locally-built paths after a build.

## Required environment

| Variable | Used by | Description |
|---|---|---|
| `NIXCACHE_REPO` | all | GitHub `owner/repo` for the OCI cache |
| `GITHUB_TOKEN` | all | GitHub token for GHCR auth |
| `NIXCACHE_SIGNING_KEY_FILE` | uploader | Path to nix cache signing key (optional) |
