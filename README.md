# Caustic OS

NixOS-based embedded operating system for the eCube energy storage solution.

## Development

```sh
nix develop   # enter dev shell with linters and tools
nix flake check
```

## Releases

CalVer (`YYYY.MM.N`). A release PR is auto-opened on `main` and bumps
`version.txt`. Merging it tags the merge commit. The dev image builds
immediately (environment `dev`); the prod image build waits for approval in
the `prod` environment.

Required GitHub environments and secrets:

- `dev`: `COSIGN_PRIVATE_KEY`, `COSIGN_PASSWORD`
- `prod` (add required reviewers): `COSIGN_PRIVATE_KEY`, `COSIGN_PASSWORD`,
  `SECUREBOOT_DB_KEY`, `SECUREBOOT_DB_CERT`

Artifacts:

- Dev: `ghcr.io/stargrid-systems/caustic-os-dev:<version>`
- Prod: `ghcr.io/stargrid-systems/caustic-os:<version>`

## Secure Boot

Image builds sign the UKI and systemd-boot when Secure Boot keys are present.
Without keys, builds succeed but produce unsigned artifacts.

### Generating keys

Use `sbctl` (included in the dev shell):

```sh
sudo sbctl create-keys
```

This places keys under `/usr/share/secureboot/keys/{db,KEK,PK}/`. The flake
auto-detects the `db` keys at that location.

To use a non-default location, set `CAUSTIC_SECUREBOOT_KEYS`:

```sh
export CAUSTIC_SECUREBOOT_KEYS=/path/to/keys/db
```

### Building signed images

Pure evaluation cannot see filesystem paths outside the flake, so pass
`--impure` to enable signing:

```sh
nix build --impure .#packages.aarch64-linux.productionImage
```

### Enrolling keys on a device

After flashing the image, enroll the keys in UEFI firmware on each device:

```sh
sbctl enroll-keys --microsoft \
  /usr/share/secureboot/keys/PK/PK.crt \
  /usr/share/secureboot/keys/KEK/KEK.crt \
  /usr/share/secureboot/keys/db/db.crt
```

## License

Released under the [GNU Affero General Public License v3 or later](./LICENSE)
(AGPL-3.0-or-later).
