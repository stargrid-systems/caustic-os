{
  config,
  ...
}:
{
  config = {
    boot.initrd.systemd.repart.enable = true;

    systemd.repart.partitions = {
      "30-usr-verity-b" = {
        Type = "usr-verity";
        SizeMinBytes = "64M";
        SizeMaxBytes = "64M";
        Label = "_empty";
        ReadOnly = 1;
      };
      "32-usr-b" = {
        Type = "usr";
        SizeMinBytes = "1536M";
        SizeMaxBytes = "1536M";
        Label = "_empty";
        ReadOnly = 1;
      };
    };

    systemd.sysupdate = {
      enable = true;
      reboot.enable = true;

      transfers = {
        "10-uki" = {
          Transfer.Verify = "no";
          Source = {
            Type = "directory-file";
            Path = "/var/lib/caustic-ota/staging";
            MatchPattern = "${config.boot.uki.name}_@v.efi";
          };
          Target = {
            Type = "regular-file";
            Path = "/EFI/Linux";
            PathRelativeTo = "esp";
            MatchPattern = "${config.boot.uki.name}_@v+@l-@d.efi ${config.boot.uki.name}_@v+@l.efi ${config.boot.uki.name}_@v.efi";
            Mode = "0644";
            TriesLeft = 3;
            TriesDone = 0;
            InstancesMax = 2;
          };
        };

        "20-usr-verity" = {
          Transfer.Verify = "no";
          Source = {
            Type = "directory-file";
            Path = "/var/lib/caustic-ota/staging";
            MatchPattern = "${config.system.image.id}_@v_@u.verity";
          };
          Target = {
            Type = "partition";
            Path = "auto";
            MatchPattern = "verity-@v";
            MatchPartitionType = "usr-verity";
            ReadOnly = 1;
          };
        };

        "22-usr" = {
          Transfer.Verify = "no";
          Source = {
            Type = "directory-file";
            Path = "/var/lib/caustic-ota/staging";
            MatchPattern = "${config.system.image.id}_@v_@u.usr";
          };
          Target = {
            Type = "partition";
            Path = "auto";
            MatchPattern = "usr-@v";
            MatchPartitionType = "usr";
            ReadOnly = 1;
          };
        };
      };
    };
  };
}
