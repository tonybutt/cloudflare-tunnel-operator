{
  projectRootFile = "flake.nix";

  programs = {
    deadnix = {
      enable = true;
      priority = 0;
    };
    nixfmt = {
      enable = true;
      priority = 1;
    };
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
