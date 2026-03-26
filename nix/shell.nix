{
  pkgs,
  rustToolchain,
  hooks,
  gitHooksShellHook,
  gitHooksPackages,
}:

pkgs.mkShell {
  name = "cloudflare-tunnel-operator";

  buildInputs =
    gitHooksPackages
    ++ hooks
    ++ [
      rustToolchain
      pkgs.kubectl
      pkgs.kustomize
      pkgs.gh
      pkgs.git
      pkgs.kind
      pkgs.cargo-tarpaulin
      pkgs.cargo-audit
    ];

  shellHook = ''
    ${gitHooksShellHook}
  '';
}
