{
  pkgs,
  self,
  lib,
}:
let
  devImage = self.nixosConfigurations.devImage;

  bootFiles = devImage.config.system.build.bootFiles;

  overlays = devImage.config.hardware.deviceTree.overlays;
  overlayNames = map (o: o.name) overlays;

  configTxt = builtins.readFile "${bootFiles}/config.txt";
in
pkgs.runCommand "dt-overlay-check"
  {
    inherit configTxt;
    expectedOverlays = lib.concatStringsSep " " overlayNames;
  }
  ''
    failures=0

    for name in $expectedOverlays; do
      if ! grep -q "dtoverlay=$name" <<< "$configTxt"; then
        echo "ERROR: config.txt missing dtoverlay=$name"
        failures=$((failures + 1))
      fi
      if [ ! -f "${bootFiles}/overlays/$name.dtbo" ]; then
        echo "ERROR: missing ${bootFiles}/overlays/$name.dtbo"
        failures=$((failures + 1))
      fi
    done

    if [ "$failures" -gt 0 ]; then
      echo "$failures DT overlay check(s) failed"
      exit 1
    fi

    touch $out
  ''
