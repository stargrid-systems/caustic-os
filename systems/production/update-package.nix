{
  config,
  lib,
  pkgs,
  ...
}:
let
  inherit (config.system) build;
  inherit (config.system.image) version id;
in
{
  config = {
    system.build.updatePackage =
      let
        finalImage = build.image.override { split = true; };
        verityImgAttrs = builtins.fromJSON (builtins.readFile "${finalImage}/repart-output.json");
        # repart-output.json partitions are ordered by partition number:
        # [ esp, store-verity, store, persist ]
        usrAttrs = builtins.elemAt verityImgAttrs 2;
        verityAttrs = builtins.elemAt verityImgAttrs 1;
        usrUuid = usrAttrs.uuid;
        verityUuid = verityAttrs.uuid;

        updateFiles = [
          {
            name = "${id}_${version}.efi";
            path = "${build.uki}/${config.system.boot.loader.ukiFile}";
          }
          {
            name = "${id}_${version}_${verityUuid}.verity";
            path = "${finalImage}/${config.image.baseName}.verity.raw";
          }
          {
            name = "${id}_${version}_${usrUuid}.usr";
            path = "${finalImage}/${config.image.baseName}.usr.raw";
          }
        ];

        createHash =
          { name, path }:
          lib.concatStringsSep "  " [
            (builtins.hashFile "sha256" path)
            name
          ];
      in
      pkgs.linkFarm "${id}-update-package" (
        updateFiles
        ++ [
          {
            name = "${id}_${version}.img";
            path = "${finalImage}/${config.image.baseName}.raw";
          }
          {
            name = "SHA256SUMS";
            path = pkgs.writeText "sha256sums" (lib.concatLines (map createHash updateFiles));
          }
        ]
      );
  };
}
