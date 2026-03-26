{
  pkgs,
  package,
  nix2container,
  tag ? "latest",
}:

nix2container.buildImage {
  name = "ghcr.io/tonybutt/cloudflare-tunnel-operator";
  inherit tag;

  config = {
    entrypoint = [ "${package}/bin/cloudflare-tunnel-operator" ];
  };

  layers = [
    (nix2container.buildLayer {
      deps = [ package ];
    })
  ];
}
