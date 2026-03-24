{
  pkgs,
  package,
  nix2container,
}:

nix2container.buildImage {
  name = "ghcr.io/tonybutt/cloudflare-tunnel-operator";
  tag = "latest";

  config = {
    entrypoint = [ "${package}/bin/cloudflare-tunnel-operator" ];
  };

  layers = [
    (nix2container.buildLayer {
      deps = [ package ];
    })
  ];
}
