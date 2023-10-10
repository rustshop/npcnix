{
  description = "Control your NixOS instances system configuration from a centrally managed location.";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-23.05";
    flake-utils.url = "github:numtide/flake-utils";
    flakebox = {
      url = "github:rustshop/flakebox?rev=b07a9f3d17d400464210464e586f76223306f62d";
    };
  };

  outputs = { self, nixpkgs, flake-utils, flakebox }: {
    nixosModules = {
      npcnix = import ./modules/npcnix.nix { inherit self; };
      default = self.nixosModules.npcnix;
    };

    nixosConfigurations =
      let
        system = "x86_64-linux";
        pkgs = import nixpkgs {
          inherit system;
        };
      in
      {
        basic = nixpkgs.lib.nixosSystem {
          inherit system pkgs;

          modules = [

            ({ modulesPath, ... }: {
              imports = [ "${modulesPath}/virtualisation/amazon-image.nix" ];
              ec2.hvm = true;


              system.stateVersion = "22.11";
            })

            self.nixosModules.default
          ];
        };
      };
  }
  // flake-utils.lib.eachDefaultSystem (system:
    let
      pkgs = import nixpkgs {
        inherit system;
      };
      lib = pkgs.lib;

      projectName = "npcnix";

      flakeboxLib = flakebox.lib.${system} {
        config = { };
      };

      buildPaths = [
        "Cargo.toml"
        "Cargo.lock"
        ".cargo"
        "src"
        "README.md"
      ];

      buildSrc = flakeboxLib.filterSubPaths {
        root = builtins.path {
          name = projectName;
          path = ./.;
        };
        paths = buildPaths;
      };

      multiBuild =
        (flakeboxLib.craneMultiBuild { }) (craneLib':
          let
            craneLib = (craneLib'.overrideArgs {
              pname = projectName;
              src = buildSrc;
              buildInputs = [
                pkgs.openssl

              ];
              nativeBuildInputs = [
                pkgs.pkg-config


              ];
            });
          in
          {
            npcnix = craneLib.buildPackage { };
          });

      npcnixPkgWrapped = pkgs.writeShellScriptBin "npcnix" ''
        exec env \
          NPCNIX_AWS_CLI=''${NPCNIX_AWS_CLI:-${pkgs.awscli2}/bin/aws} \
          NPCNIX_NIXOS_REBUILD=''${NPCNIX_NIXOS_REBUILD:-${pkgs.nixos-rebuild}/bin/nixos-rebuild} \
          PATH="${pkgs.git}/bin:$PATH" \
          ${multiBuild.npcnix}/bin/npcnix "$@"
      '';
    in
    {
      packages = {
        default = npcnixPkgWrapped;
        npcnix-unwrapped = multiBuild.npcnix;
        npcnix = npcnixPkgWrapped;
        install = pkgs.writeShellScriptBin "npcnix-install" ''
          set -e
          npcnix_swapfile="/npcnix-install-swapfile"

          function cleanup() {
            if [ -e "$npcnix_swapfile" ]; then
              >&2 echo "Cleaning up temporary swap file..."
              ${pkgs.util-linux}/bin/swapoff "$npcnix_swapfile" || true
              ${pkgs.coreutils}/bin/rm -f "$npcnix_swapfile" || true
            fi
          }
          # clean unconditionally, in case we left over something in a previous run, etc.
          trap cleanup EXIT

          if [ "$(${pkgs.util-linux}/bin/swapon --noheadings --raw | ${pkgs.coreutils}/bin/wc -l )" = "0" ] ; then
            >&2 echo "No swap detected. Creating a temporary swap file..."
            if [ ! -e "$npcnix_swapfile" ]; then
              # it has been experimentally verified, that 2G should be enough
              # bootstrap even on AWS EC2 t3.nano instances
              ${pkgs.util-linux}/bin/fallocate -l 2G "$npcnix_swapfile" || true
            fi
            chmod 600 "$npcnix_swapfile" && \
              ${pkgs.util-linux}/bin/mkswap "$npcnix_swapfile" && \
              ${pkgs.util-linux}/bin/swapon "$npcnix_swapfile" || \
              true
          fi

          ${npcnixPkgWrapped}/bin/npcnix install "$@"
          cleanup
        '';
      };

      checks = {
        nixosConfiguration = self.nixosConfigurations.basic.config.system.build.toplevel;
        npcnix = self.packages.${system}.npcnix;
        install = self.packages.${system}.install;
      };

      devShells = {
        default = flakeboxLib.mkDevShell { };
      };
    }
  );
}

