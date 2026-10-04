//! Starting up: what the server is told, and what it refuses to start without.

use std::io::IsTerminal as _;
use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use nemlig::{Client, Credentials};

use crate::render::OutputLimits;
use crate::server::Mode;

/// An MCP server over the nemlig.com API.
#[derive(Debug, Parser)]
#[command(name = "nemlig-mcp", version, about, long_about = None)]
pub struct Args {
    /// The nemlig.com account to sign in as.
    #[arg(long, env = "NEMLIG_USERNAME")]
    pub username: Option<String>,

    /// Read the password from a file instead of NEMLIG_PASSWORD.
    ///
    /// There is deliberately no --password: a command line is readable by every process on
    /// the machine and is kept in the shell's history.
    #[arg(long, env = "NEMLIG_PASSWORD_FILE", value_name = "PATH")]
    pub password_file: Option<PathBuf>,

    /// Also serve the tools that change the basket.
    #[arg(long, env = "NEMLIG_MCP_ALLOW_WRITES")]
    pub allow_writes: bool,

    /// Where to keep the session between runs.
    #[arg(long, env = "NEMLIG_SESSION_FILE", value_name = "PATH")]
    pub session_file: Option<PathBuf>,

    /// Sign in afresh on every start instead of keeping a session file.
    #[arg(long, conflicts_with = "session_file")]
    pub no_session_file: bool,

    /// Point the website calls somewhere else.
    #[arg(long, env = "NEMLIG_BASE_URL", value_name = "URL")]
    pub base_url: Option<String>,

    /// Point the search calls somewhere else.
    #[arg(long, env = "NEMLIG_SEARCH_BASE_URL", value_name = "URL")]
    pub search_base_url: Option<String>,

    /// How long one request may take, in seconds.
    #[arg(long, default_value_t = 30, value_name = "SECONDS")]
    pub timeout: u64,

    /// How many rows a listing returns before it truncates.
    #[arg(long, default_value_t = 100, value_name = "COUNT")]
    pub max_items: usize,

    /// Print the tool schemas and exit, without signing in.
    #[arg(long)]
    pub list_tools: bool,

    /// Check the credentials and exit.
    #[arg(long)]
    pub check: bool,
}

/// Why the server would not start.
#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    /// Something has to be set before the server can do anything.
    #[error("{what} is not set. {how}")]
    Missing {
        /// The variable or flag that is missing.
        what: &'static str,
        /// What to do about it.
        how: &'static str,
    },

    /// A password file was named but could not be read.
    #[error("could not read the password from {path}")]
    PasswordFile {
        /// The path that failed.
        path: String,
        /// The underlying failure.
        #[source]
        source: std::io::Error,
    },

    /// The client could not be built from what was given.
    #[error("the client could not be built")]
    Client(#[source] nemlig::Error),

    /// nemlig.com would not accept the credentials.
    #[error("nemlig.com did not accept the credentials")]
    Rejected(#[source] Box<nemlig::Error>),

    /// The tool schemas could not be printed.
    #[error("could not write the tool schemas")]
    Schemas(#[from] serde_json::Error),

    /// No cache directory could be worked out for the session file.
    #[error("no cache directory could be found; pass --session-file or --no-session-file")]
    NoCacheDir,
}

impl Args {
    /// Which tools to serve.
    pub fn mode(&self) -> Mode {
        Mode {
            writes: self.allow_writes,
        }
    }

    /// How much one tool may return.
    pub fn limits(&self) -> OutputLimits {
        OutputLimits {
            max_items: self.max_items,
            ..OutputLimits::default()
        }
    }

    /// Build the client, or say what is missing.
    pub fn client(&self) -> Result<Client, StartupError> {
        let username = present(self.username.as_deref()).ok_or(StartupError::Missing {
            what: "NEMLIG_USERNAME",
            how: "Set it to the email address the nemlig.com account signs in with.",
        })?;
        let password = self.password()?;

        let mut builder = Client::builder()
            .credentials(Credentials::new(username, password))
            .timeout(Duration::from_secs(self.timeout));

        if let Some(base) = present(self.base_url.as_deref()) {
            builder = builder.base_url(base);
        }
        if let Some(base) = present(self.search_base_url.as_deref()) {
            builder = builder.search_base_url(base);
        }
        if let Some(path) = self.session_path()? {
            builder = builder.session_file(path);
        }

        builder.build().map_err(StartupError::Client)
    }

    /// Where the password comes from: a file, or the environment.
    fn password(&self) -> Result<String, StartupError> {
        if let Some(path) = &self.password_file {
            let raw =
                std::fs::read_to_string(path).map_err(|source| StartupError::PasswordFile {
                    path: path.display().to_string(),
                    source,
                })?;
            return Ok(raw.trim_end_matches(['\r', '\n']).to_owned());
        }
        present(std::env::var("NEMLIG_PASSWORD").ok().as_deref()).ok_or(StartupError::Missing {
            what: "NEMLIG_PASSWORD",
            how: "Set it to the account's password, or name a file with --password-file. \
                  There is no flag for the password itself.",
        })
    }

    fn session_path(&self) -> Result<Option<PathBuf>, StartupError> {
        if self.no_session_file {
            return Ok(None);
        }
        if let Some(path) = &self.session_file {
            return Ok(Some(path.clone()));
        }
        let dirs = directories::ProjectDirs::from("com", "nemlig", "nemlig")
            .ok_or(StartupError::NoCacheDir)?;
        Ok(Some(dirs.cache_dir().join("session.json")))
    }
}

/// Treat an empty value as an absent one.
///
/// A client that substitutes configuration into the environment sends the variable whether
/// or not the user filled it in, so an empty string arrives where nothing was meant.
fn present(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// Send the log to stderr, since stdout is the protocol.
pub fn init_tracing() {
    // NEMLIG_MCP_LOG wins over RUST_LOG: a client that sets RUST_LOG for its own sake would
    // otherwise flood this server's stderr with something nobody asked it for.
    let filter = tracing_subscriber::EnvFilter::try_from_env("NEMLIG_MCP_LOG")
        .or_else(|_| tracing_subscriber::EnvFilter::try_from_default_env())
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("nemlig_mcp=info,nemlig=warn"));

    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .with_env_filter(filter)
        .try_init();
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use clap::CommandFactory as _;

    #[test]
    fn the_argument_parser_is_well_formed() {
        Args::command().debug_assert();
    }

    #[test]
    fn there_is_no_password_argument() {
        // A command line is readable by every process on the machine. If this ever starts
        // failing, someone added the convenience and the secret with it.
        let found = Args::command()
            .get_arguments()
            .any(|arg| arg.get_long() == Some("password"));
        assert!(!found, "--password must not exist");
    }

    #[test]
    fn an_empty_value_counts_as_nothing_being_set() {
        assert_eq!(present(Some("  ")), None);
        assert_eq!(present(Some("")), None);
        assert_eq!(present(None), None);
        assert_eq!(present(Some(" a@b.c ")), Some("a@b.c".to_owned()));
    }

    #[test]
    fn writes_are_off_unless_asked_for() {
        let args = Args::try_parse_from(["nemlig-mcp"]).expect("parses");
        assert_eq!(args.mode(), Mode { writes: false });

        let args = Args::try_parse_from(["nemlig-mcp", "--allow-writes"]).expect("parses");
        assert_eq!(args.mode(), Mode { writes: true });
    }

    #[test]
    fn a_missing_account_names_the_variable_to_set() {
        let args = Args::try_parse_from(["nemlig-mcp"]).expect("parses");
        let err = args.client().expect_err("refuses to start");
        assert!(err.to_string().contains("NEMLIG_USERNAME"), "got {err}");
    }
}
