# Hook declarations for cachix/git-hooks.nix
# Each custom hook references a script from nix/pre-commit.nix via hookBin.
{
  hookBin,
  treefmt,
  rustToolchain,
}:
{
  # ── Pre-commit hooks ───────────────────────────────────────────────
  treefmt = {
    enable = true;
    name = "Format All";
    entry = "${treefmt}/bin/treefmt --fail-on-change";
    pass_filenames = false;
    stages = [ "pre-commit" ];
  };
  clippy = {
    enable = true;
    packageOverrides = {
      cargo = rustToolchain;
      clippy = rustToolchain;
    };
    stages = [ "pre-commit" ];
  };
  generate-manifests = {
    enable = true;
    name = "generate-manifests";
    entry = hookBin.generate-manifests;
    files = "(deploy/.*\\.yaml|src/crd\\.rs)$";
    pass_filenames = false;
    stages = [ "pre-commit" ];
  };
  cargo-audit = {
    enable = true;
    name = "cargo-audit";
    entry = "${hookBin.cargo-audit}";
    pass_filenames = false;
    stages = [ "pre-commit" ];
  };
  commitizen.enable = true;

  # ── Pre-push hooks ─────────────────────────────────────────────────
  e2e-tests = {
    enable = true;
    name = "e2e-tests";
    entry = hookBin.e2e-tests;
    pass_filenames = false;
    always_run = true;
    stages = [ "pre-push" ];
  };
}
