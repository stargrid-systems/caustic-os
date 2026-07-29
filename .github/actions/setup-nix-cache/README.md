# nixcache-oci

Nix binary cache backed by GHCR (OCI registry). Originally forked from
[cmspam/nixcache-oci](https://github.com/cmspam/nixcache-oci), now a
first-party component.

## How it works

NARs are stored as content-addressed OCI blobs under
`ghcr.io/<repo>/nix-cache`. A single `cache-index` tag holds a JSON
manifest mapping every store hash to its narinfo text and NAR blob digest.

The action installs Determinate Nix, starts a local proxy as a substituter,
and automatically uploads locally-built paths at the end of the job via a
post hook. No manual save step is needed.

## Files

- `main.js` - setup: installs uv, starts proxy, installs Determinate Nix
- `post.js` - save: uploads locally-built paths (runs automatically)
- `src/nixcache/` - Python package:
  - `proxy.py` - HTTP proxy bridging Nix binary cache protocol to GHCR
  - `upload.py` - exports and uploads store paths
  - `gc.py` - prunes stale index entries
  - `oci.py` - OCI registry client (token, blob, manifest operations)
  - `index.py` - cache-index model with optimistic concurrency
  - `nar.py` - NAR export, narinfo generation, self-check
  - `config.py` - env-based configuration and utilities

## Inputs

| Name | Default | Description |
|---|---|---|
| `public-key` | (required) | Public key for validating signed NARs |
| `out-link` | `result` | Nix output symlink to record as a GC root |
| `save` | `true` | Upload locally-built paths at end of job |

## Required environment

| Variable | Description |
|---|---|
| `NIX_CACHE_PRIVATE_KEY` | Nix cache signing key (secret) |
| `GITHUB_TOKEN` | GitHub token for GHCR auth |
