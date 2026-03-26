# Hook declarations for cachix/git-hooks.nix
# Uses prek for priority-based ordering (lower = runs first, same = parallel).
# Each custom hook references a script from nix/pre-commit.nix via hookBin.
{
  hookBin,
  treefmt,
  rustToolchain,
}:
{
  # ── Pre-commit: 0 — fast checks (parallel) ────────────────────────
  typos = {
    enable = true;
    priority = 0;
  };
  check-merge-conflicts = {
    enable = true;
    priority = 0;
  };
  check-added-large-files = {
    enable = true;
    priority = 0;
  };
  flake-checker = {
    enable = true;
    priority = 0;
  };
  statix = {
    enable = true;
    priority = 0;
  };

  # ── Pre-commit: 10 — formatting ───────────────────────────────────
  treefmt = {
    enable = true;
    name = "Format All";
    entry = "${treefmt}/bin/treefmt --fail-on-change";
    pass_filenames = false;
    stages = [ "pre-commit" ];
    priority = 10;
  };

  # ── Pre-commit: 20 — lint / audit (parallel) ──────────────────────
  clippy = {
    enable = true;
    packageOverrides = {
      cargo = rustToolchain;
      clippy = rustToolchain;
    };
    stages = [ "pre-commit" ];
    priority = 20;
  };
  cargo-audit = {
    enable = true;
    name = "cargo-audit";
    entry = "${hookBin.cargo-audit}";
    pass_filenames = false;
    stages = [ "pre-commit" ];
    priority = 20;
  };

  # ── Pre-commit: 30 — codegen ──────────────────────────────────────
  generate-manifests = {
    enable = true;
    name = "generate-manifests";
    entry = hookBin.generate-manifests;
    files = "(deploy/.*\\.yaml|src/crd\\.rs)$";
    pass_filenames = false;
    stages = [ "pre-commit" ];
    priority = 30;
  };

  # ── Commit-msg ────────────────────────────────────────────────────
  commitizen.enable = true;

  # ── Pre-push: 0 — container build ────────────────────────────────
  nix-build = {
    enable = true;
    name = "nix-build";
    entry = "nix build .#container";
    pass_filenames = false;
    always_run = true;
    stages = [ "pre-push" ];
    priority = 0;
  };

  # ── Pre-push: 10 — e2e tests ─────────────────────────────────────
  e2e-tests = {
    enable = true;
    name = "e2e-tests";
    entry = hookBin.e2e-tests;
    pass_filenames = false;
    always_run = true;
    stages = [ "pre-push" ];
    priority = 10;
  };
}
