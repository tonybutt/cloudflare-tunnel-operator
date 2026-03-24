{ pkgs, treefmt-nix }:

treefmt-nix.lib.evalModule pkgs {
  projectRootFile = "flake.nix";

  programs = {
    nixfmt.enable = true;
    rustfmt.enable = true;
    prettier = {
      enable = true;
      includes = [
        "*.md"
        "*.json"
        "*.yaml"
        "*.yml"
      ];
    };
  };
}
