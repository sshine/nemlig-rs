# Credentials for the live API tests.
#
# secretspec.toml declares which secrets the live suite needs; where the values come from
# is each developer's own choice (system keyring locally, workflow environment in CI), so
# nothing secret is committed and `just live-test` works the same in both places.
{ ... }:
{
  perSystem =
    { pkgs, ... }:
    {
      devshells.default.packages = [ pkgs.secretspec ];
    };
}
