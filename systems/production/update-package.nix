{
  config,
  lib,
  pkgs,
  ...
}:
let
  inherit (config.system) build;
  inherit (config.system.image) version id;

  finalImage = build.image.override { split = true; };
  verityImgAttrs = builtins.fromJSON (builtins.readFile "${finalImage}/repart-output.json");
  # repart-output.json partitions are ordered by partition number:
  # [ esp, store-verity, store, persist ]
  usrAttrs = builtins.elemAt verityImgAttrs 2;
  verityAttrs = builtins.elemAt verityImgAttrs 1;
  usrUuid = usrAttrs.uuid;
  verityUuid = verityAttrs.uuid;

  baseName = config.image.baseName;

  decompress =
    name: sourcePath:
    pkgs.runCommand name
      {
        nativeBuildInputs = [ pkgs.zstd ];
      }
      ''
        zstd -d -f ${sourcePath} -o $out
      '';

  verityDecompressed = decompress "${id}-${version}-verity" "${finalImage}/${baseName}.verity.raw.zst";
  usrDecompressed = decompress "${id}-${version}-usr" "${finalImage}/${baseName}.usr.raw.zst";
  imgDecompressed = decompress "${id}-${version}-img" "${finalImage}/${baseName}.raw.zst";
in
{
  config = {
    system.build.updatePackage =
      let
        updateFiles = [
          {
            name = "${id}_${version}.efi";
            path = "${build.uki}/${config.system.boot.loader.ukiFile}";
          }
          {
            name = "${id}_${version}_${verityUuid}.verity";
            path = verityDecompressed;
          }
          {
            name = "${id}_${version}_${usrUuid}.usr";
            path = usrDecompressed;
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
            path = imgDecompressed;
          }
          {
            name = "SHA256SUMS";
            path = pkgs.writeText "sha256sums" (lib.concatLines (map createHash updateFiles));
          }
        ]
      );
  };
}
