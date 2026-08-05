{
  pkgs,
  self,
}:
let
  inherit (pkgs) lib;
  inherit (pkgs.stdenv.hostPlatform) system;
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

    environment.systemPackages = [
      pkgs.curl
      pkgs.sshpass
    ];
  };

  nodes.caustic = { pkgs, ... }: {
    imports = [
      self.nixosModules.caustic
      self.nixosModules.aperture
      self.nixosModules.dropbear
    ];

    boot.kernelPackages = pkgs.linuxPackages;

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

    users.users.root.hashedPassword = "";

    services = {
      aperture = {
        enable = true;
        package = self.packages.${system}.aperture;
        httpsAddr = "[::]:443";
      };
      dropbear = {
        enable = true;
        allowRootLogin = true;
        allowPasswordAuth = true;
        allowEmptyPasswords = true;
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
        router.succeed(
            f"sshpass -p ''' ssh -o StrictHostKeyChecking=no "
            f"-o UserKnownHostsFile=/dev/null root@{ip} 'echo SSH_OK'"
        )

    with subtest("aperture HTTP on port 80"):
        router.succeed(f"curl -s -o /dev/null http://{ip}/")

    with subtest("aperture HTTPS on port 443"):
        router.succeed(f"curl -sk -o /dev/null https://{ip}/")
  '';
}
