//! The server itself: what it holds, which tools it serves, and what it tells a client.

use rmcp::ServerHandler;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{Implementation, ServerCapabilities, ServerConfig};
use rmcp::tool_handler;

use crate::render::OutputLimits;

/// Which groups of tools to serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mode {
    /// Serve the tools that change the basket.
    pub writes: bool,
}

/// One nemlig.com account, served over MCP.
#[derive(Debug, Clone)]
pub struct NemligServer {
    pub(crate) client: nemlig::Client,
    pub(crate) limits: OutputLimits,
    mode: Mode,
    tool_router: ToolRouter<Self>,
}

impl NemligServer {
    /// Serve one account.
    pub fn new(client: nemlig::Client, mode: Mode, limits: OutputLimits) -> Self {
        Self {
            client,
            limits,
            mode,
            tool_router: Self::router(mode),
        }
    }

    /// The tools this mode serves.
    ///
    /// Composed rather than filtered: a tool that was never built cannot be reached by name
    /// either, so what `list_all` reports is exactly what can be called.
    pub fn router(mode: Mode) -> ToolRouter<Self> {
        let mut router = Self::catalogue_read_router();
        router.merge(Self::basket_read_router());
        router.merge(Self::orders_read_router());
        router.merge(Self::account_read_router());

        if mode.writes {
            router.merge(Self::basket_write_router());
        }

        router
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for NemligServer {
    fn get_info(&self) -> ServerConfig {
        // Named explicitly: the default is `Implementation::from_build_env`, which reads the
        // environment rmcp itself was built in, so a server that takes it announces itself as
        // "rmcp" and every server in a client's list looks the same.
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(instructions(self.mode))
    }
}

/// What holds for every tool, said once here rather than in each description.
fn instructions(mode: Mode) -> String {
    let mut text = String::from(
        "This server talks to one nemlig.com account, the Danish online supermarket. \
         Everything it returns is in Danish: product and category names, labels, campaign \
         names, units such as Stk. and kr./Kg., and nutrition keys such as Energi. Prices \
         are in kroner and depend on the account's delivery slot and zone, which \
         nemlig_get_account reports. Product numbers are the strings search returns; the \
         order id in the history is a different number from the order number the website \
         prints. nemlig.com publishes no rate limit, so the client paces itself and a run \
         of calls takes proportionally longer. Available delivery slots cannot be browsed \
         or changed from here: only the one already booked can be read.",
    );

    if mode.writes {
        text.push_str(
            " The basket can be changed. Quantities are absolute: setting a product to 3 \
             leaves 3 of it whatever was there before, and setting it to 0 removes the \
             line. nemlig.com has no separate remove verb. Nothing here can place an \
             order or spend money; that is not built into this server at all.",
        );
    } else {
        text.push_str(
            " This server was started without --allow-writes, so it serves no tool that \
             changes the basket. Nothing here can place an order or spend money.",
        );
    }

    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_instructions_say_what_the_mode_withholds() {
        let reading = instructions(Mode { writes: false });
        assert!(reading.contains("--allow-writes"));

        let writing = instructions(Mode { writes: true });
        assert!(writing.contains("absolute"));
        assert!(writing.contains("0 removes"));
    }

    #[test]
    fn both_modes_say_that_nothing_can_place_an_order() {
        for mode in [Mode { writes: false }, Mode { writes: true }] {
            assert!(
                instructions(mode).contains("place an order"),
                "the mode has to say it"
            );
        }
    }
}
