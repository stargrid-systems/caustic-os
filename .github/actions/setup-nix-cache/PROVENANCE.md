# nixcache-oci

Vendored from <https://github.com/cmspam/nixcache-oci> at commit
`03250ab2193af7b38a10a27ae44b4aab7dd1cbe0`.

- `lib/cache-builder.sh` — build, filter and upload pipeline. Source it
  from CI steps and call `start_self_substituter`,
  `find_locally_built_paths`, `export_paths_directly`, `upload_to_oci`.
- `proxy/main.py` — local HTTP proxy that bridges the nix binary cache
  protocol to GHCR OCI blobs. Started in the background by the composite
  action so the nix daemon treats it as a regular substituter.

To update: copy the two files from upstream at a new pinned commit and
re-run `nix flake check`. There is no nixpkgs dependency here, only
bash, python3 stdlib, curl, jq and xz.
