//! An MCP server over the nemlig.com API, speaking JSON-RPC on stdin and stdout.

use std::process::ExitCode;

use clap::Parser as _;
use rmcp::ServiceExt as _;
use rmcp::transport::stdio;

use nemlig_mcp::cli::{self, Args, StartupError};
use nemlig_mcp::server::NemligServer;

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();
    cli::init_tracing();

    match run(args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            report(&err);
            // Distinct from 1 so a supervisor can tell a refused start from a crash.
            ExitCode::from(2)
        }
    }
}

async fn run(args: Args) -> Result<(), StartupError> {
    let mode = args.mode();

    // Neither of these serves anything, so both may write to stdout, and --list-tools needs
    // no credentials: the surface depends on the mode alone.
    if args.list_tools {
        let tools = NemligServer::router(mode).list_all();
        println!("{}", serde_json::to_string_pretty(&tools)?);
        return Ok(());
    }

    let client = args.client()?;

    if args.check {
        client
            .account()
            .context()
            .await
            .map_err(|err| StartupError::Rejected(Box::new(err)))?;
        println!(
            "credentials accepted for {}",
            client.credentials().username()
        );
        return Ok(());
    }

    let server = NemligServer::new(client, mode, args.limits());
    match server.serve(stdio()).await {
        Ok(service) => {
            if let Err(err) = service.waiting().await {
                tracing::error!(%err, "the connection ended badly");
            }
        }
        Err(err) => tracing::error!(%err, "could not start serving"),
    }

    Ok(())
}

/// Report a refused start on stderr, since stdout is the protocol even when nothing served.
fn report(err: &StartupError) {
    eprintln!("nemlig-mcp: {err}");
    let mut source = std::error::Error::source(err);
    while let Some(cause) = source {
        eprintln!("  caused by: {cause}");
        source = cause.source();
    }
}
