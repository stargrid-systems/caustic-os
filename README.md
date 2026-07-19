# Caustic OS

NixOS-based embedded operating system for the eCube energy storage solution.

## Development

```sh
nix develop   # enter dev shell with linters and tools
nix flake check
```

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

Cosign (keyless, via Fulcio) remains the trust mechanism for OTA artifacts in
the registry. Secure Boot protects the on-device boot chain.

## License

Released under the [GNU Affero General Public License v3 or later](./LICENSE)
(AGPL-3.0-or-later).
