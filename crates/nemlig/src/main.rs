//! The `nemlig` command-line client.
//!
//! A thin shell over the library: one subcommand per call, human-readable output by
//! default and `--json` for the wire types. It keeps the session in a file so running it
//! twice in a row does not sign in twice.

mod cli;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args = cli::parse();

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("nemlig: could not start: {error}");
            return ExitCode::from(2);
        }
    };

    match runtime.block_on(cli::run(args)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("nemlig: {error}");
            // Two for a configuration problem the user can fix before trying again, one
            // for a call that went out and came back wrong, so a script can tell them
            // apart without reading the message.
            ExitCode::from(error.exit_code())
        }
    }
}
