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

## Installation

Each release publishes an OCI artifact to GHCR containing a full disk image
and A/B update components. The full image (`.img`) is used for initial
installation. The other files (`.usr`, `.verity`, `.efi`) are for in-place
updates handled by `systemd-sysupdate`.

### Prerequisites

Install `oras` to pull artifacts from GHCR:

```sh
nix profile install nixpkgs#oras
```

### Pull the artifact

```sh
oras login ghcr.io -u <github-username> --password-stdin <<< "<pat>"
mkdir caustic-os && cd caustic-os
oras pull ghcr.io/stargrid-systems/caustic-os:<version>
sha256sum -c SHA256SUMS
```

### Flash to CM4 eMMC

The CM4 has onboard eMMC flash. To write to it you need `rpiboot` to put
the module into USB mass storage mode.

1. Install `rpiboot` on the host:

   ```sh
   nix shell nixpkgs#rpiboot
   ```

2. Fit the jumper on the carrier board to force USB boot mode. Connect
   the CM4 to the host via USB.

3. Run rpiboot:

   ```sh
   sudo rpiboot
   ```

4. The eMMC appears as a block device (typically `/dev/sdX`). Flash the
   image:

   ```sh
   sudo dd if=caustic-os_<version>.img of=/dev/sdX bs=4M conv=fsync status=progress
   sync
   ```

5. Remove the jumper, disconnect USB, and power-cycle the board.

### Flash to SD card

For a CM4 configured for SD card boot:

```sh
sudo dd if=caustic-os_<version>.img of=/dev/sdX bs=4M conv=fsync status=progress
sync
```

### First boot

The root filesystem is tmpfs. Persistent state lives on the `persist`
partition. The system is read-only and protected by dm-verity.

For production images, enroll the secure boot keys (see below).

## Secure Boot

Production images are signed with Secure Boot keys when keys are
configured during the build. After flashing a signed image, enroll the
keys in UEFI firmware on each device:

```sh
sbctl enroll-keys --microsoft \
  /usr/share/secureboot/keys/PK/PK.crt \
  /usr/share/secureboot/keys/KEK/KEK.crt \
  /usr/share/secureboot/keys/db/db.crt
```

## License

Released under the [GNU Affero General Public License v3 or later](./LICENSE)
(AGPL-3.0-or-later).
