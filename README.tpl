# {{crate}}

{{readme}}

## Command-line client

The same crate installs a `nemlig` binary.

```bash
cargo install nemlig
export NEMLIG_USERNAME=someone@example.com
export NEMLIG_PASSWORD=...        # there is no --password flag, by design

nemlig search mælk -n 5
nemlig product 5070417
nemlig basket
nemlig basket set 5070417 2       # absolute: two of them, not two more
nemlig orders
nemlig delivery
```

`--json` prints the API's own documents instead of columns. The session is kept under the
user's cache directory, so running it twice does not sign in twice; `--no-session-file`
turns that off.

A library consumer that does not want the binary's dependencies takes
`default-features = false, features = ["rustls-tls"]`.

## MCP server

`nemlig-mcp` serves this API to Claude Code and other MCP clients over stdio. It reads
only until told otherwise, and nothing in it can place an order.

```bash
nix profile install github:sshine/nemlig-rs#nemlig-mcp
```

Prebuilt binaries for Linux, macOS and Windows are on the [releases page]. See
[crates/nemlig-mcp/README.md] for the tool reference and how to point a client at it.

[crates/nemlig-mcp/README.md]: crates/nemlig-mcp/README.md
[releases page]: https://github.com/sshine/nemlig-rs/releases

## Development

Everything runs inside `nix develop` (or `direnv allow`, which enters it for you).

```bash
just         # list the recipes
just ci      # what CI runs: fmt-check, lint, test, doc, readme-check, build
just test    # the mocked suite; no network, no sleeping
```

`README.md` is generated from the crate documentation, so edit `crates/nemlig/src/lib.rs`
and run `just readme`. A pre-push hook checks the two agree.

### Testing against the real API

The live suite is `#[ignore]`d and needs a real nemlig.com account. The credentials are
declared in `secretspec.toml` and resolved through [secretspec], so nothing secret is
committed and each developer picks where the values live.

```bash
secretspec config global init      # once: choose a provider, e.g. the system keyring
secretspec set NEMLIG_USERNAME
secretspec set NEMLIG_PASSWORD
just secrets-check                 # reports what is still missing
just live-test
```

Everything in it reads, except one test that adds a product to the real basket and takes
it out again. That one runs only when `NEMLIG_TEST_BASKET` and `NEMLIG_TEST_PRODUCT_ID`
are set, and it puts the quantity back where it found it.

On Linux the keyring provider talks to the Secret Service over D-Bus, so it needs
gnome-keyring or KWallet running and unlocked. Any other provider works just as well;
`secretspec.toml` does not care which.

[secretspec]: https://secretspec.dev

## License

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))
