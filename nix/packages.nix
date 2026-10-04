# `packages.default` is the library and its CLI: that is what someone installing this
# reaches for. The MCP server is reached by name, which is also what an MCP client
# configuration would spell out.
{ ... }:
{
  perSystem =
    { pkgs, lib, ... }:
    rec {
      checks.nemlig = packages.default;
      checks.nemlig-mcp = packages.nemlig-mcp;

      packages.default = pkgs.callPackage ./_package.nix { };
      packages.nemlig-mcp = pkgs.callPackage ./_package.nix { crate = "nemlig-mcp"; };

      apps.nemlig = {
        type = "app";
        program = lib.getExe packages.default;
      };
      apps.nemlig-mcp = {
        type = "app";
        program = lib.getExe packages.nemlig-mcp;
      };
      apps.default = apps.nemlig;
    };
}
