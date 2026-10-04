# The build definition, shared by this flake's packages and by the overlay.
#
# Taking `pkgs` as an argument (rather than closing over this flake's own) is what
# lets the overlay build against the consumer's nixpkgs, so downstream can override
# and cross-compile it.
#
# `crate` selects the workspace member. The description and the binary come from that
# member's own manifest, so adding a third member needs nothing here.
{
  lib,
  rustPlatform,
  cacert,
  crate ? "nemlig",
  ...
}:
let
  manifest = lib.importTOML (../crates + "/${crate}/Cargo.toml");
in
rustPlatform.buildRustPackage {
  pname = crate;
  # Read rather than repeated: a release bumps one place, and this cannot fall behind it.
  version = (lib.importTOML ../Cargo.toml).workspace.package.version;

  # reqwest's rustls backend loads the system trust store when a client is constructed,
  # not when a request is made, so every test that builds a Client fails in the sandbox
  # with "No CA certificates were loaded from the system". The mock tests only ever talk
  # to loopback over plain HTTP; this is purely to get past client construction.
  SSL_CERT_FILE = "${cacert}/etc/ssl/certs/ca-bundle.crt";

  # Naming the inputs explicitly keeps target/ and .direnv/ out of the store, and
  # means an unrelated edit does not invalidate the build.
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../crates
      ../README.md
    ];
  };
  cargoLock.lockFile = ../Cargo.lock;

  cargoBuildFlags = [
    "--package"
    crate
  ];

  # Without this the check phase runs the whole workspace's tests, so each package's
  # check would fail on a bug in the other one.
  cargoTestFlags = [
    "--package"
    crate
  ];

  meta = {
    inherit (manifest.package) description;
    license = with lib.licenses; [
      mit
      asl20
    ];
    # Both members install a binary, so both are runnable. The library crate carries the
    # CLI as a second target rather than as a crate of its own.
    mainProgram = crate;
  };
}
