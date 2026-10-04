# nemlig

An API client for [nemlig.com], the Danish online supermarket.

Unofficial and unaffiliated. nemlig.com publishes no API; this crate drives the one its
own website uses, recovered from captured traffic, and it can stop working whenever the
website changes.

### What it covers

Signing in, searching the catalogue, reading a product, reading and changing the basket,
reading past orders, and reading the delivery slot and address the account is set up
with. Everything here either reads, or changes the basket.

Placing an order is not part of the default build. It exists behind the `place-order`
feature and a second, runtime opt-in, and the two are separate on purpose: nothing in a
default `cargo add nemlig` can spend money.

### Signing in

```rust
use nemlig::{Client, Credentials};

let client = Client::new(Credentials::new("someone@example.com", "hunter2"))?;

let results = client.search().query("kakaomælk").take(5).send().await?;
for product in &results.products.products {
    println!("{:>9}  {:<40} {:>8.2} kr", product.id, product.name, product.price);
}
```

There is no explicit login step. The first call signs in, and later calls renew the
five-minute bearer token from the session cookies rather than from the password.
\[`Client::sign_in`\] exists for when you want the handshake's own answer, such as to
check a password.

### Keeping the session

Signing in costs five requests, and nemlig.com's authentication cookie lasts a year.
A process that runs once per invocation, such as a command-line tool, should keep it:

```rust
let client = Client::builder()
    .credentials(Credentials::new("someone@example.com", "hunter2"))
    .session_file("/home/me/.cache/nemlig/session.json")
    .build()?;
```

The file is written so only its owner can read it, but it is still a credential.

### The basket

nemlig.com has no endpoint that removes a basket line. Quantities are absolute, and
setting one to zero is what removes it, so the crate offers a single verb:

```rust
let id = "5070417".parse()?;
client.basket().set_quantity(&id, 3).await?;  // three of them, not three more
client.basket().set_quantity(&id, 0).await?;  // gone
```

### Pacing

nemlig.com states no rate limit and sends no headers describing one, but it sits behind
Cloudflare, which answers a burst with a challenge rather than a number. The client
therefore spaces its own requests out, five a second by default, and treats a 429's
`Retry-After` as the only authority on how long to wait. Reads are retried; writes are
not, since the client cannot tell from the method whether one went through.

### Danish fields

Responses are in Danish and stay that way: category and label names, campaign
descriptions, units such as `Stk.` and `kr./Kg.`, and nutrition keys such as `Energi`.
Numbers inside strings use the comma as the decimal separator, which
[`DanishNumber`](types::DanishNumber) reads.

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

## License

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

[crates/nemlig-mcp/readme.md]: crates/nemlig-mcp/README.md
[nemlig.com]: https://www.nemlig.com
[releases page]: https://github.com/sshine/nemlig-rs/releases
[secretspec]: https://secretspec.dev
