{
  description = "Cloudflare Tunnel Operator for Kubernetes";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    nix2container = {
      url = "github:nlewo/nix2container";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    git-hooks = {
      url = "github:cachix/git-hooks.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      nix2container,
      treefmt-nix,
      git-hooks,
    }:
    let
      system = "x86_64-linux";
      overlays = [ rust-overlay.overlays.default ];
      pkgs = import nixpkgs { inherit system overlays; };

      rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

      package = import ./nix/package.nix { inherit pkgs rustToolchain; };
      container = import ./nix/container.nix {
        inherit pkgs package;
        nix2container = nix2container.packages.${system}.nix2container;
      };
      treefmt = import ./nix/treefmt.nix { inherit pkgs treefmt-nix; };
      pre-commit = import ./nix/git-hooks.nix {
        inherit
          system
          pkgs
          git-hooks
          treefmt
          rustToolchain
          ;
      };
    in
    {
      formatter.${system} = treefmt.config.build.wrapper;

      packages.${system} = {
        default = package;
        container = container;
      };

      devShells.${system}.default = import ./nix/shell.nix {
        inherit pkgs rustToolchain;
        git-hooks = pre-commit;
      };

      checks.${system} = {
        formatting = treefmt.config.build.check self;
        pre-commit = pre-commit;
      };
    };
}
