{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.boot.native-rpi;

  rpiFw = pkgs.raspberrypifw;
  version = config.system.image.version;
  imageId = config.system.image.id;
  toplevel = config.system.build.toplevel;

  allKernelParams = lib.concatStringsSep " " config.boot.kernelParams;

  initrd = config.system.build.initialRamdisk;

  closureInfo = pkgs.buildPackages.closureInfo {
    rootPaths = [ toplevel ];
  };

  rootSquashfs =
    pkgs.runCommand "root-squashfs"
      {
        nativeBuildInputs = [ pkgs.squashfsTools ];
      }
      ''
        set -euo pipefail
        rootDir=$PWD/rootfs
        mkdir -p \
          $rootDir/nix/store \
          $rootDir/nix/var/nix/profiles \
          $rootDir/etc \
          $rootDir/boot/a \
          $rootDir/boot/b \
          $rootDir/{run,tmp,var,dev,proc,sys,persist,home,mnt,opt,srv,root} \
          $rootDir/var/lib/{aperture,caustic-ota,dropbear,nixos,systemd} \
          $rootDir/var/log/journal \
          $rootDir/var/{db,empty,spool} \
          $rootDir/var/lib/lastlog

        xargs -I % cp -a --reflink=auto % -t $rootDir/nix/store/ < ${closureInfo}/store-paths

        ln -sf ${toplevel} $rootDir/nix/var/nix/profiles/system
        ln -sf /run/current-system/sw/bin $rootDir/bin
        ln -sf /run/current-system/sw/sbin $rootDir/sbin
        ln -sf /run/current-system/sw/lib $rootDir/lib
        ln -sf /run/current-system/sw/lib64 $rootDir/lib64
        ln -sf /run/lock $rootDir/var/lock
        ln -sf /run $rootDir/var/run

        cp -rs ${config.system.build.etc}/etc/. $rootDir/etc/
        ${pkgs.systemd}/bin/systemd-machine-id-setup --root $rootDir --print 2>/dev/null || true

        SOURCE_DATE_EPOCH=0 mksquashfs $rootDir $out \
          -all-root -no-hardlinks \
          -b 1048576 -comp zstd -Xcompression-level 6 \
          -processors $NIX_BUILD_CORES -root-mode 0755 -noappend
      '';

  verityArtifacts =
    pkgs.runCommand "verity-artifacts"
      {
        nativeBuildInputs = [ pkgs.cryptsetup ];
      }
      ''
        set -euo pipefail
        mkdir -p $out

        sqfsSize=$(stat -c %s ${rootSquashfs})
        dataBlocks=$(( (sqfsSize + 4095) / 4096 ))
        paddedSize=$(( dataBlocks * 4096 ))
        cp ${rootSquashfs} $out/usr.bin
        chmod 644 $out/usr.bin
        if [ $sqfsSize -ne $paddedSize ]; then
          truncate -s $paddedSize $out/usr.bin
        fi

        output=$(veritysetup format --no-superblock $out/usr.bin $out/hash.bin)
        rootHash=$(echo "$output" | grep '^Root hash:' | awk '{print $3}')
        salt=$(echo "$output" | grep '^Salt:' | awk '{print $2}')

        cat $out/hash.bin >> $out/usr.bin
        rm $out/hash.bin

        printf 'data_blocks=%s\nroot_hash=%s\nsalt=%s\n' "$dataBlocks" "$rootHash" "$salt" > $out/verity.txt
      '';

  configTxt = pkgs.writeText "config.txt" ''
    arm_64bit=1
    arm_boost=1
    enable_uart=1
    uart_2ndstage=1
    enable_gic=1
    disable_commandline_tags=1
    disable_overscan=1
    gpu_mem=16
    dtoverlay=miniuart-bt
    kernel=Image
    initramfs initrd followkernel
    cmdline=cmdline.txt
    start_file=start4.elf
    fixup_file=fixup4.dat
  '';

  autobootTxt = pkgs.writeText "autoboot.txt" ''
    [all]
    tryboot_a_b=1
    boot_partition=1
  '';

  bootFiles =
    pkgs.runCommand "boot-files"
      {
        nativeBuildInputs = [ pkgs.coreutils ];
      }
      ''
        set -euo pipefail
        mkdir -p $out/overlays
        cp ${config.boot.kernelPackages.kernel}/Image $out/Image
        cp ${initrd}/initrd $out/initrd
        cp ${configTxt} $out/config.txt
        cp ${autobootTxt} $out/autoboot.txt
        cp ${rpiFw}/share/raspberrypi/boot/start4.elf $out/
        cp ${rpiFw}/share/raspberrypi/boot/fixup4.dat $out/
        cp ${rpiFw}/share/raspberrypi/boot/start.elf $out/
        cp ${rpiFw}/share/raspberrypi/boot/fixup.dat $out/
        cp ${rpiFw}/share/raspberrypi/boot/bcm2711-rpi-cm4.dtb $out/
        cp ${rpiFw}/share/raspberrypi/boot/overlays/miniuart-bt.dtbo $out/overlays/

        source ${verityArtifacts}/verity.txt
        sectors=$(( data_blocks * 8 ))

        for slot in a b; do
          if [ "$slot" = a ]; then
            dev=/dev/mmcblk0p5
          else
            dev=/dev/mmcblk0p6
          fi
          printf '%s\n' \
            "init=${toplevel}/init dm-mod.create=\"vroot,,0,ro,0 ''${sectors} verity 1 ''${dev} ''${dev} 4096 4096 ''${data_blocks} ''${data_blocks} sha256 ''${root_hash} ''${salt} 1 restart_on_corruption\" ${allKernelParams}" \
            > "$out/cmdline-$slot.txt"
        done
      '';

  makeBootFat =
    slot:
    pkgs.runCommand "boot-$slot.img"
      {
        nativeBuildInputs = [
          pkgs.dosfstools
          pkgs.mtools
        ];
      }
      ''
        img=$out
        truncate -s 256M $img
        mkfs.vfat -F 32 -n BOOT $img
        mcopy -i $img -s ${bootFiles}/* ::
        mcopy -i $img -o ${bootFiles}/cmdline-${slot}.txt ::cmdline.txt
        fsck.vfat -vn $img
        mtype -i $img ::start4.elf >/dev/null || { echo "ERROR: start4.elf missing from boot image"; exit 1; }
      '';

  bootFatImageA = makeBootFat "a";
  bootFatImageB = makeBootFat "b";

  persistImg =
    pkgs.runCommand "persist.img"
      {
        nativeBuildInputs = [ pkgs.e2fsprogs ];
      }
      ''
        sourceDir=$PWD/persist-source
        mkdir -p $sourceDir/etc
        touch $sourceDir/etc/machine-id
        truncate -s 1G $out
        mke2fs -t ext4 -L persist -b 4096 -F -d $sourceDir $out
      '';

  diskImage =
    pkgs.runCommand "${imageId}-${version}.img"
      {
        nativeBuildInputs = [ pkgs.util-linux ];
      }
      ''
        set -euo pipefail
        img=$PWD/disk.img

        usrSize=$(stat -c %s ${verityArtifacts}/usr.bin)
        usrBlocks=$(( (usrSize + 511) / 512 ))

        bootSize=524288
        persistSize=2097152

        bootAStart=2048
        bootBStart=$(( bootAStart + bootSize ))
        persistStart=$(( bootBStart + bootSize ))
        extStart=$(( persistStart + persistSize ))

        usrAStart=$(( extStart + 2048 ))
        usrBStart=$(( usrAStart + usrBlocks + 2048 ))
        totalSectors=$(( usrBStart + usrBlocks + 2048 ))
        totalSectors=$(( ((totalSectors + 1023) / 1024) * 1024 ))

        truncate -s $(( totalSectors * 512 )) $img

        sfdisk $img << EOF
        label: dos
        unit: sectors

        start=$bootAStart,  size=$bootSize,     type=0c, bootable
        start=$bootBStart,  size=$bootSize,     type=0c
        start=$persistStart, size=$persistSize,  type=83
        start=$extStart,    size=$(( totalSectors - extStart )), type=5
        start=$usrAStart,   size=$usrBlocks,     type=83
        start=$usrBStart,   size=$usrBlocks,     type=83
        EOF

        dd if=${bootFatImageA} of=$img bs=512 seek=$bootAStart conv=notrunc 2>/dev/null
        dd if=${bootFatImageB} of=$img bs=512 seek=$bootBStart conv=notrunc 2>/dev/null
        dd if=${persistImg} of=$img bs=512 seek=$persistStart conv=notrunc 2>/dev/null
        dd if=${verityArtifacts}/usr.bin of=$img bs=512 seek=$usrAStart conv=notrunc 2>/dev/null

        cp $img $out
      '';

  bootTar = pkgs.runCommand "boot.tar" { } ''
    tar --transform='s,^\./,,' -cf $out -C ${bootFiles} .
  '';

  updateFiles = [
    {
      name = "${imageId}_${version}.usr";
      path = "${verityArtifacts}/usr.bin";
    }
    {
      name = "${imageId}_${version}_verity.txt";
      path = "${verityArtifacts}/verity.txt";
    }
    {
      name = "${imageId}_${version}_boot.tar";
      path = bootTar;
    }
    {
      name = "${imageId}_${version}.img";
      path = diskImage;
    }
  ];

  createHash = { name, path }: "${builtins.hashFile "sha256" path}  ${name}";
in
{
  imports = [ ./slot.nix ];

  options.boot.native-rpi = {
    enable = lib.mkEnableOption "native Raspberry Pi boot (no UEFI)";
  };

  config = lib.mkIf cfg.enable {
    boot = {
      loader = {
        grub.enable = false;
        generic-extlinux-compatible.enable = false;
        systemd-boot.enable = false;
      };

      initrd = {
        includeDefaultModules = false;
        systemd.enable = false;

        postDeviceCommands = ''
          mkdir -p /mnt-root/lower /mnt-root/upper
          mount -t squashfs /dev/dm-0 /mnt-root/lower -o ro
          mount -t tmpfs tmpfs /mnt-root/upper
        '';
      };

      kernelParams = [
        "console=ttyAMA0,115200"
        "console=tty1"
        "earlycon=pl011,mmio32,0xfe201000"
        "ignore_loglevel"
        "panic=30"
        "dm-mod.waitfor=/dev/mmcblk0"
        "systemd.show_status=1"
        "systemd.log_level=info"
      ];
    };

    systemd.settings.Manager = {
      RuntimeWatchdogSec = "off";
      ShutdownWatchdogSec = "10min";
    };

    fileSystems = {
      "/" = {
        device = "overlay";
        fsType = "overlay";
        options = [
          "lowerdir=/lower"
          "upperdir=/upper/upper"
          "workdir=/upper/work"
        ];
        neededForBoot = true;
      };
      "/persist" = {
        device = "/dev/disk/by-label/persist";
        fsType = "ext4";
        neededForBoot = true;
      };
    };

    system.build = {
      image = diskImage;
      updatePackage = pkgs.linkFarm "${imageId}-update-package" (
        updateFiles
        ++ [
          {
            name = "SHA256SUMS";
            path = pkgs.writeText "sha256sums" (lib.concatLines (map createHash updateFiles));
          }
        ]
      );
      inherit
        rootSquashfs
        verityArtifacts
        bootFiles
        diskImage
        ;
      inherit initrd;
    };
  };
}
