# Caustic OS

NixOS-based embedded operating system for the eCube energy storage solution.

## Releases

CalVer (`YYYY.MM.N`). A release PR is auto-opened on `main` and bumps
`version.txt`. Merging it tags the merge commit. The dev image builds
immediately (environment `dev`); the prod image build waits for approval in
the `prod` environment.

Artifacts:

- Dev: `ghcr.io/stargrid-systems/caustic-os-dev:<version>`
- Prod: `ghcr.io/stargrid-systems/caustic-os:<version>`

## Secure Boot

Production images are signed with Secure Boot keys. After flashing an image,
enroll the keys in UEFI firmware on each device:

```sh
sbctl enroll-keys --microsoft \
  /usr/share/secureboot/keys/PK/PK.crt \
  /usr/share/secureboot/keys/KEK/KEK.crt \
  /usr/share/secureboot/keys/db/db.crt
```

## License

Released under the [GNU Affero General Public License v3 or later](./LICENSE)
(AGPL-3.0-or-later).
