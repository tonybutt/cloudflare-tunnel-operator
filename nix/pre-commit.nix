{ pkgs, rustToolchain }:
[
  (pkgs.writeShellApplication {
    name = "generate-manifests";
    runtimeInputs = [
      rustToolchain
      pkgs.kustomize
      pkgs.git
    ];
    text = builtins.readFile ../scripts/generate.sh;
  })
]
