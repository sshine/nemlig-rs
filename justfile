# Default recipe: list available commands
default:
    @just --list

# Format all code (Rust + Nix + Markdown + TOML + YAML)
fmt:
    treefmt

# Check formatting (Rust + Nix + Markdown + TOML + YAML)
fmt-check:
    treefmt --fail-on-change --no-cache

# Run clippy lints
lint:
    cargo clippy --all-targets --all-features -- -D warnings

# Run all tests
test:
    cargo test --all-features

secretspec_reason := "nemlig-rs live API test suite"

# Configure a provider once with `secretspec config global init`, then `secretspec set`
# each secret named in secretspec.toml; `just secrets-check` reports what is still
# missing. The reason is recorded by providers that keep an audit log, and secretspec
# refuses to read anything without one.
#
# Which provider is not decided here. Locally it is whatever the developer configured;
# CI sets SECRETSPEC_PROVIDER=env, since a runner has no keyring but does have the
# workflow's environment. Either way resolution goes through secretspec, so a missing
# secret fails with the same message in both places.
#
# The tests are #[ignore]d so `just test` skips them; --ignored is what opts in.
#
# One thread: the account has a single basket and a single session, so two tests running
# at once would see each other's writes.
[doc("Run the tests that talk to the real API, with credentials from secretspec")]
live-test *args='':
    secretspec run --reason {{quote(secretspec_reason)}} -- \
        cargo test --all-features --test live -- --ignored --nocapture --test-threads 1 {{args}}

# Report which live-test credentials are missing from the configured provider
secrets-check:
    secretspec check --reason {{quote(secretspec_reason)}}

# Run the CLI against the real API, with credentials from secretspec
cli *args='':
    secretspec run --reason {{quote(secretspec_reason)}} -- \
        cargo run --quiet -p nemlig -- {{args}}

# Run the MCP server over stdio, with credentials from secretspec
mcp *args='':
    secretspec run --reason {{quote(secretspec_reason)}} -- \
        cargo run --quiet -p nemlig-mcp -- {{args}}

# Print the MCP tool schemas, which needs no credentials
mcp-tools *args='':
    cargo run --quiet -p nemlig-mcp -- --list-tools {{args}}

# Build release
build:
    cargo build --release --all-features

# Generate documentation
doc *args='':
    cargo doc --no-deps --all-features {{args}}

readme_args := "--project-root crates/nemlig --input src/lib.rs --template ../../README.tpl"

# Regenerate README.md from README.tpl and the crate docs
readme:
    cargo readme {{readme_args}} | mdformat - > README.md

# Check README.md is in sync with README.tpl and the crate docs
readme-check:
    cargo readme {{readme_args}} | mdformat - | diff - README.md

# Assert the release tag names the version cargo would publish
check-version version:
    @pkgid="$(cargo pkgid -p nemlig)"; crate="v${pkgid##*#}"; \
    if [ "$crate" != "{{version}}" ]; then \
        echo "tag {{version}} does not match crate version $crate" >&2; exit 1; \
    fi
    @# The workspace dependency on the library names the version too, and cannot inherit
    @# it. Left behind it would resolve against an older release from the registry rather
    @# than the sources beside it, which packaging would accept without complaint.
    @dep="v$(grep -oP '(?<=^nemlig = \{ version = ")[^"]+' Cargo.toml)"; \
    if [ "$dep" != "{{version}}" ]; then \
        echo "tag {{version}} does not match the workspace nemlig dependency $dep" >&2; exit 1; \
    fi

# Verify Cargo.lock is in step with the manifests
lock-check:
    cargo metadata --locked --format-version 1 > /dev/null

# Verify every member packages cleanly, the unpublished ones included
#
# `cargo package` checks manifests that `cargo build` never looks at, so this is the only
# step that catches a dependency missing the version requirement publishing needs.
#
# --allow-dirty because otherwise this refuses to run with uncommitted work, which would
# make `just ci` useful only after committing. CI checks out clean, so it packages the
# same tree either way; the flag only stops the refusal.
package-check:
    cargo package --locked --workspace --allow-dirty

# Run CI checks locally
ci: fmt-check lint test doc readme-check build lock-check package-check
    @echo "All CI checks passed!"

# Clean build artifacts
clean:
    cargo clean
