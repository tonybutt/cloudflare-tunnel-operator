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
  (pkgs.writeShellApplication {
    name = "e2e-tests";
    runtimeInputs = [ rustToolchain ];
    text = ''
      if [ -f .env ]; then
        set -a
        # shellcheck source=/dev/null
        source .env
        set +a
      fi
      cargo test --test e2e -- --ignored --test-threads=1
    '';
  })
]
