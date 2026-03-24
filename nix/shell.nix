{
  pkgs,
  rustToolchain,
  git-hooks,
}:

pkgs.mkShell {
  name = "cloudflare-tunnel-operator";

  packages = with pkgs; [
    rustToolchain
    kubectl
    kustomize
    gh
    git
    kind
  ];

  shellHook = ''
    ${git-hooks.shellHook}
  '';
}
