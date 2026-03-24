{
  system,
  pkgs,
  git-hooks,
  treefmt,
  rustToolchain,
}:

git-hooks.lib.${system}.run {
  src = ../.;
  hooks = {
    treefmt = {
      enable = true;
      package = treefmt.config.build.wrapper;
      entry = "${treefmt.config.build.wrapper}/bin/treefmt --fail-on-change";
    };
    clippy = {
      enable = true;
      packageOverrides = {
        cargo = rustToolchain;
        clippy = rustToolchain;
      };
    };
    commitizen.enable = true;
  };
}
