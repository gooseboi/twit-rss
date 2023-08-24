# Most of this is from https://fasterthanli.me/series/building-a-rust-service-with-nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs = {
        nixpkgs.follows = "nixpkgs";
        flake-utils.follows = "flake-utils";
      };
    };
    crane = {
      url = "github:ipetkov/crane";
      inputs = {
        nixpkgs.follows = "nixpkgs";
        rust-overlay.follows = "rust-overlay";
        flake-utils.follows = "flake-utils";
      };
    };
  };

  nixConfig.bash-prompt-prefix  = "\[nix-develop\]$ ";

  outputs = { self, nixpkgs, flake-utils, rust-overlay, crane}:
      flake-utils.lib.eachDefaultSystem (system:
          let
            overlays = [ (import rust-overlay) ];
            pkgs = import nixpkgs {
              inherit system overlays;
            };

            nativeBuildInputs = with pkgs; [ rustToolchain pkg-config openssl ];
            buildInputs = with pkgs; [ geckodriver firefox dive just ];

            rustToolchain = pkgs.pkgsBuildHost.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

            craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;
            src = craneLib.cleanCargoSource ./.;

            commonArgs = {
              inherit src buildInputs nativeBuildInputs;
            };

            cargoArtifacts = craneLib.buildDepsOnly commonArgs;
            bin = craneLib.buildPackage (commonArgs // {
              inherit cargoArtifacts;
            });
            dockerImage = pkgs.dockerTools.buildImage {
              name = "twit-rss";
              tag = "latest";
              copyToRoot = [ bin ];
              config = {
                Cmd = [ "${bin}/bin/twit-rss" ];
              };
            };
          in
          {
            packages = {
              inherit bin dockerImage;
              default = bin;
            };
            devShells.default = pkgs.mkShell {
              inherit nativeBuildInputs buildInputs;
            };
          });
}
