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
  partitionBySplitName =
    splitName:
    let
      expected = "${baseName}.${splitName}.raw";
      hasSplit =
        p: p ? split_path && builtins.isString p.split_path && builtins.baseNameOf p.split_path == expected;
      matches = builtins.filter hasSplit verityImgAttrs;
    in
    if builtins.length matches == 1 then
      builtins.head matches
    else
      throw "repart-output: expected exactly one partition with split_path=\"${expected}\", found ${toString (builtins.length matches)}";
  usrAttrs = partitionBySplitName "usr";
  verityAttrs = partitionBySplitName "verity";
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
          {
            name = "${id}_${version}.img";
            path = imgDecompressed;
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
            name = "SHA256SUMS";
            path = pkgs.writeText "sha256sums" (lib.concatLines (map createHash updateFiles));
          }
        ]
      );
  };
}
