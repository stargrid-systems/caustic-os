{
  pkgs,
  self,
}:
let
  inherit (pkgs) lib;
  inherit (pkgs.stdenv.hostPlatform) system;

  testPrivKey = pkgs.writeText "e2e-test-key" ''
    -----BEGIN OPENSSH PRIVATE KEY-----
    b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
    QyNTUxOQAAACBBesLOYSOkM3vsk+8qqLn2VGMfgnZsQ23tJ0DleagayQAAAJhdX7HhXV+x
    4QAAAAtzc2gtZWQyNTUxOQAAACBBesLOYSOkM3vsk+8qqLn2VGMfgnZsQ23tJ0DleagayQ
    AAAEAh5ONfGceiW3I0xjIQSmAZizjDHoGPIur9PGQCs0NpckF6ws5hI6Qze+yT7yqoufZU
    Yx+CdmxDbe0nQOV5qBrJAAAAEGNhdXN0aWMtZTJlLXRlc3QBAgMEBQ==
    -----END OPENSSH PRIVATE KEY-----
  '';

  testPubKey = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIEF6ws5hI6Qze+yT7yqoufZUYx+CdmxDbe0nQOV5qBrJ caustic-e2e-test";
in
pkgs.testers.runNixOSTest {
  name = "caustic-e2e-test";

  nodes.router = { pkgs, ... }: {
    virtualisation.vlans = [ 1 ];

    networking = {
      firewall.enable = false;
      useDHCP = false;
      interfaces.eth0.ipv4.addresses = lib.mkForce [
        {
          address = "10.1.0.1";
          prefixLength = 24;
        }
      ];
    };

    services.dnsmasq = {
      enable = true;
      settings = {
        interface = "eth0";
        bind-interfaces = true;
        dhcp-range = "10.1.0.100,10.1.0.200,255.255.255.0,12h";
        dhcp-option = [
          "option:router,10.1.0.1"
          "option:dns-server,10.1.0.1"
        ];
      };
    };

    environment = {
      systemPackages = [
        pkgs.curl
        pkgs.openssh
      ];
      etc."ssh/test_key".source = "${testPrivKey}";
    };
  };

  nodes.caustic = { ... }: {
    imports = [
      self.nixosModules.caustic
      self.nixosModules.kernel
      self.nixosModules.aperture
      self.nixosModules.dropbear
    ];

    virtualisation = {
      vlans = [ 1 ];
      cores = 2;
      memorySize = 1024;
    };

    caustic = {
      hardening.enable = true;
      networking.enable = true;
      users.enable = true;
    };

    services = {
      aperture = {
        enable = true;
        package = self.packages.${system}.aperture;
        httpsAddr = "[::]:443";
      };
      dropbear = {
        enable = true;
        allowRootLogin = true;
      };
    };

    networking = {
      useDHCP = lib.mkForce true;
      interfaces.eth0.ipv4.addresses = lib.mkForce [ ];
      firewall.allowedTCPPorts = [
        80
        443
      ];
    };

    system.activationScripts.authorizedKeys = ''
      mkdir -p /root/.ssh
      echo '${testPubKey}' > /root/.ssh/authorized_keys
      chmod 700 /root/.ssh
      chmod 600 /root/.ssh/authorized_keys
    '';

    system.stateVersion = "26.05";
  };

  testScript = ''
    caustic.start()
    router.start()

    caustic.wait_for_unit("multi-user.target")
    router.wait_for_unit("dnsmasq.service")

    with subtest("caustic gets DHCP lease"):
        caustic.wait_until_succeeds("ip -4 addr show eth0 | grep '10.1.0.'")
        ip = caustic.succeed(
            "ip -4 addr show eth0 | awk '/inet /{print $2}' | cut -d/ -f1"
        ).strip()

    with subtest("SSH from router to caustic"):
        router.succeed("install -m 600 /etc/ssh/test_key /root/test_key")
        router.succeed(
            f"ssh -i /root/test_key -o StrictHostKeyChecking=no "
            f"-o UserKnownHostsFile=/dev/null root@{ip} 'echo SSH_OK'"
        )

    with subtest("aperture HTTP on port 80"):
        router.succeed(f"curl -s -o /dev/null http://{ip}/")

    with subtest("aperture HTTPS on port 443"):
        router.succeed(f"curl -sk -o /dev/null https://{ip}/")
  '';
}
