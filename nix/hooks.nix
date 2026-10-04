{ inputs, ... }:
{
  imports = [ inputs.hk-nix.flakeModules.default ];
  perSystem =
    {
      config,
      pkgs,
      lib,
      ...
    }:
    let

      # Uses tools by absolute store path: the `nix flake check` hk-check sandbox
      # runs hooks without the devshell PATH, so a relative `treefmt` is not found.
      treefmt = lib.getExe config.treefmt.build.wrapper;

      betterleaks = lib.getExe pkgs.betterleaks;

      # Called by store path rather than via `cargo readme`, so argv needs fixing.
      cargo-readme = "${lib.getExe' pkgs.cargo-readme "cargo-readme"} readme";
      readmeArgs = "--project-root crates/nemlig --input src/lib.rs --template ../../README.tpl";

      # cargo-readme and mdformat disagree about placement of reference definitions
      # (the placement of `[foo]: https://...`) because mdformat sees a bigger picture
      # than README.tpl. To run both cargo-readme and mdformat, they're run in series.
      mdformat = config.treefmt.settings.formatter.mdformat.command;
      readme = "${cargo-readme} ${readmeArgs} | ${mdformat} -";
    in
    {
      hk-nix.settings.hooks = {
        "pre-commit" = {
          fix = true;
          stash = "git";
          steps = {
            treefmt.check = "${treefmt} --fail-on-change --no-cache {{files}}";
            treefmt.fix = "${treefmt} {{files}}";
            betterleaks.check = "${betterleaks} dir --redact --no-banner --config .betterleaks.toml {{files}}";
          };
        };

        "pre-push".steps = {
          deadnix.glob = "*.nix";
          deadnix.check = "${lib.getExe pkgs.deadnix} --fail {{files}}";
          clippy.check = "cargo clippy --all-targets --all-features -- -D warnings";
          readme.check = "${readme} | diff - README.md";
          readme.fix = "${readme} > README.md";
          lock-check.check = "cargo metadata --locked --format-version 1 > /dev/null";
        };

        "commit-msg".steps.conventional.builtin = config.hk-nix.builtins.check_conventional_commit;
      };
    };
}
