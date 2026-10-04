{ ... }:
{
  flake.overlays.default = final: _prev: {
    nemlig = final.callPackage ./_package.nix { };
    nemlig-mcp = final.callPackage ./_package.nix { crate = "nemlig-mcp"; };
  };
}
