# nemlig-mcp

An MCP server over the [nemlig.com](https://www.nemlig.com) API, the Danish online
supermarket. It speaks JSON-RPC on stdin and stdout.

Unofficial and unaffiliated. nemlig.com publishes no API; this drives the one its own
website uses, and it can stop working whenever the website changes.

**Nothing here can place an order or spend money.** The library keeps order placement
behind a feature this crate never enables, so it is not compiled in at all.

## Install

```bash
cargo install nemlig-mcp
# or
nix profile install github:sshine/nemlig-rs#nemlig-mcp
```

Prebuilt binaries for Linux, macOS and Windows are on the
[releases page](https://github.com/sshine/nemlig-rs/releases).

## Configure

```json
{
  "mcpServers": {
    "nemlig": {
      "command": "nemlig-mcp",
      "env": {
        "NEMLIG_USERNAME": "someone@example.com",
        "NEMLIG_PASSWORD": "..."
      }
    }
  }
}
```

Add `"args": ["--allow-writes"]` to let it change the basket.

## Tools

| Mode | Tools |
| --- | --- |
| default | 9 |
| `--allow-writes` | 11 |

Always served:

| Tool | Does |
| --- | --- |
| `nemlig_search_products` | Searches the catalogue; returns products with prices, stock and offers |
| `nemlig_suggest_queries` | What the search box offers for a half-typed Danish term |
| `nemlig_get_product` | One product in full: attributes, declaration, nutrition, offers |
| `nemlig_get_basket` | Every line in the basket, and the total for the goods |
| `nemlig_list_orders` | Past orders, newest first |
| `nemlig_get_order` | One past order and every line it contained |
| `nemlig_get_account` | Postcode, delivery zone and delivery slot the session shops in |
| `nemlig_get_delivery_slot` | The delivery currently booked, if any |
| `nemlig_list_delivery_placements` | Where the driver may leave the crates |

Behind `--allow-writes`:

| Tool | Does |
| --- | --- |
| `nemlig_set_basket_quantity` | Sets how many of a product the basket holds; 0 removes |
| `nemlig_empty_basket` | Removes every line, after checking the line count still matches |

`nemlig-mcp --list-tools` prints the full schemas and needs no credentials.

## Flags

Every flag is also an environment variable, so a client that can set one or the other can
configure the server either way.

| Flag | Variable | Default |
| --- | --- | --- |
| `--username` | `NEMLIG_USERNAME` | required |
| *(none)* | `NEMLIG_PASSWORD` | required |
| `--password-file` | `NEMLIG_PASSWORD_FILE` | — |
| `--allow-writes` | `NEMLIG_MCP_ALLOW_WRITES` | off |
| `--session-file` | `NEMLIG_SESSION_FILE` | the user's cache directory |
| `--no-session-file` | — | off |
| `--base-url` | `NEMLIG_BASE_URL` | the live site |
| `--search-base-url` | `NEMLIG_SEARCH_BASE_URL` | the live gateway |
| `--timeout` | — | 30 seconds |
| `--max-items` | — | 100 rows |
| `--check` | — | — |
| `--list-tools` | — | — |

There is deliberately no `--password`: a command line is readable by every process on the
machine and is kept in the shell's history.

The log goes to stderr, since stdout is the protocol. `NEMLIG_MCP_LOG` sets the filter and
wins over `RUST_LOG`, so a client that sets `RUST_LOG` for its own sake does not flood it.

## Things worth knowing

- **One basket per account.** It is the same basket the website and the app show, so a
  write here is visible to whoever opens either next.
- **Quantities are absolute.** Setting a product to 3 leaves 3 of it whatever was there
  before, and 0 removes the line. nemlig.com has no separate remove endpoint.
- **Everything is in Danish**: product and category names, labels, campaign names, units
  such as `Stk.` and `kr./Kg.`, and nutrition keys such as `Energi`.
- **Prices depend on the delivery slot and zone**, which `nemlig_get_account` reports. Two
  accounts can get different answers to the same search.
- **Delivery slots cannot be browsed or chosen from here.** Only the one already booked
  can be read; nemlig.com exposes no endpoint for the rest.
- **No published rate limit.** The client paces itself at five requests a second and
  honours a 429's `Retry-After`, so a run of calls takes proportionally longer.

## License

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE))
- MIT license ([LICENSE-MIT](../../LICENSE-MIT))

Note that `rmcp`, which this server is built on, is Apache-2.0 only.
