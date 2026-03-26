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
      n2c = nix2container.packages.${system}.nix2container;
      container = import ./nix/container.nix {
        inherit pkgs package;
        nix2container = n2c;
      };

      treefmtEval = treefmt-nix.lib.evalModule pkgs (import ./nix/treefmt.nix);
      treefmt = treefmtEval.config.build.wrapper;

      hookPackages = import ./nix/pre-commit.nix { inherit pkgs rustToolchain; };
      hookBin = builtins.listToAttrs (
        map (drv: {
          inherit (drv) name;
          value = "${drv}/bin/${drv.name}";
        }) hookPackages
      );

      hookDefs = import ./nix/git-hooks.nix { inherit hookBin treefmt rustToolchain; };

      gitHooksCheck = git-hooks.lib.${system}.run {
        src = ./.;
        hooks = hookDefs;
      };
    in
    {
      formatter.${system} = treefmt;

      packages.${system} = {
        default = package;
        inherit container;
      };

      devShells.${system}.default = import ./nix/shell.nix {
        inherit pkgs rustToolchain;
        hooks = hookPackages;
        gitHooksShellHook = gitHooksCheck.shellHook;
        gitHooksPackages = gitHooksCheck.enabledPackages;
      };

      checks.${system} = {
        formatting = treefmtEval.config.build.check self;
      };
    };
}
