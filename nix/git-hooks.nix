{
  system,
  pkgs,
  git-hooks,
  treefmt,
  rustToolchain,
}:

let
  generate-manifests = pkgs.writeShellApplication {
    name = "generate-manifests";
    runtimeInputs = [
      rustToolchain
      pkgs.kustomize
      pkgs.git
    ];
    text = builtins.readFile ../scripts/generate.sh;
  };
in
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
    generate-manifests = {
      enable = true;
      name = "generate-manifests";
      entry = "${generate-manifests}/bin/generate-manifests";
      files = "(deploy/.*\\.yaml|src/crd\\.rs)$";
      pass_filenames = false;
    };
    commitizen.enable = true;
  };
}
