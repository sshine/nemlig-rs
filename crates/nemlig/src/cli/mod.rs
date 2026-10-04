//! Argument parsing and command dispatch.

mod render;

use std::io::IsTerminal as _;
use std::path::PathBuf;
use std::time::Duration;

use clap::{Args as ClapArgs, Parser, Subcommand};

use nemlig::{Client, Credentials, ProductId};

/// Read the command line.
pub fn parse() -> Args {
    Args::parse()
}

/// A command-line client for nemlig.com.
#[derive(Debug, Parser)]
#[command(name = "nemlig", version, about, long_about = None)]
pub struct Args {
    #[command(subcommand)]
    command: Command,

    #[command(flatten)]
    global: Global,
}

/// Settings that apply whichever subcommand runs.
#[derive(Debug, ClapArgs)]
struct Global {
    /// Print the API's own JSON instead of a table.
    #[arg(long, global = true)]
    json: bool,

    /// The nemlig.com account to sign in as.
    #[arg(short, long, global = true, env = "NEMLIG_USERNAME")]
    username: Option<String>,

    /// Read the password from a file instead of NEMLIG_PASSWORD.
    ///
    /// There is deliberately no --password: a command line is readable by every process
    /// on the machine and is kept in the shell's history.
    #[arg(long, global = true, env = "NEMLIG_PASSWORD_FILE", value_name = "PATH")]
    password_file: Option<PathBuf>,

    /// Where to keep the session between runs.
    #[arg(long, global = true, env = "NEMLIG_SESSION_FILE", value_name = "PATH")]
    session_file: Option<PathBuf>,

    /// Sign in afresh every run instead of keeping a session file.
    #[arg(long, global = true, conflicts_with = "session_file")]
    no_session_file: bool,

    /// Point the website calls somewhere else.
    #[arg(long, global = true, env = "NEMLIG_BASE_URL", value_name = "URL")]
    base_url: Option<String>,

    /// Point the search calls somewhere else.
    #[arg(
        long,
        global = true,
        env = "NEMLIG_SEARCH_BASE_URL",
        value_name = "URL"
    )]
    search_base_url: Option<String>,

    /// How long one request may take, in seconds.
    #[arg(long, global = true, default_value_t = 30, value_name = "SECONDS")]
    timeout: u64,

    /// Log what the client is doing, to stderr.
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Sign in and keep the session.
    Login {
        /// Check the password without keeping the session.
        #[arg(long)]
        check: bool,
    },
    /// Forget the kept session.
    Logout,
    /// Show which account the session belongs to, and where it delivers.
    Whoami,
    /// Search the catalogue.
    Search {
        /// What to look for. Danish terms match best.
        query: String,
        /// How many products to show.
        #[arg(short = 'n', long, default_value_t = 10)]
        number: u32,
        /// How many to skip.
        #[arg(long, default_value_t = 0)]
        skip: u32,
        /// Also show this many matching recipes.
        #[arg(long, default_value_t = 0)]
        recipes: u32,
    },
    /// Show what the search box would suggest for a half-typed term.
    Suggest {
        /// The partial term.
        query: String,
    },
    /// Show one product in full.
    Product {
        /// A product number, or the slug its page lives at.
        product: String,
    },
    /// Read or change the basket.
    Basket {
        #[command(subcommand)]
        action: Option<BasketAction>,
    },
    /// List past orders.
    Orders {
        /// How many to show.
        #[arg(short = 'n', long, default_value_t = 10)]
        number: u32,
        /// How many to skip.
        #[arg(long, default_value_t = 0)]
        skip: u32,
    },
    /// Show one past order and what it contained.
    Order {
        /// The order's numeric id, as `nemlig orders` prints it.
        id: i64,
    },
    /// Show the delivery the account has booked.
    Delivery,
    /// List where the driver may leave the crates.
    Placements,
    /// List the payment cards on the account.
    Cards,
}

#[derive(Debug, Subcommand)]
enum BasketAction {
    /// Set how many of a product the basket holds.
    Set {
        /// The product number.
        product: String,
        /// How many to hold. This is absolute, and 0 removes the line.
        quantity: u32,
    },
    /// Remove every line.
    Empty {
        /// Required: this removes everything without asking again.
        #[arg(long)]
        yes: bool,
    },
}

/// Why the command could not be run.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    /// Something the user has to set or fix before trying again.
    #[error("{0}")]
    Setup(String),

    /// The call went out and came back wrong.
    #[error(transparent)]
    Api(#[from] nemlig::Error),

    /// Output could not be written.
    #[error("could not write the output: {0}")]
    Output(#[from] std::io::Error),
}

impl CliError {
    /// What to exit with.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Setup(_) => 2,
            _ => 1,
        }
    }

    fn setup(message: impl Into<String>) -> Self {
        Self::Setup(message.into())
    }
}

type Result<T> = std::result::Result<T, CliError>;

/// Run the command the arguments name.
pub async fn run(args: Args) -> Result<()> {
    if args.global.verbose {
        init_tracing();
    }

    let client = build_client(&args.global)?;
    let json = args.global.json;

    match args.command {
        Command::Login { check } => {
            let outcome = client.sign_in().await?;
            if check {
                client.sign_out().await?;
            }
            render::sign_in(&outcome, json)
        }
        Command::Logout => {
            client.sign_out().await?;
            println!("signed out");
            Ok(())
        }
        Command::Whoami => {
            let context = client.account().context().await?;
            render::context(&context, client.credentials().username(), json)
        }
        Command::Search {
            query,
            number,
            skip,
            recipes,
        } => {
            let results = client
                .search()
                .query(query)
                .take(number)
                .skip(skip)
                .recipes(recipes)
                .send()
                .await?;
            render::search(&results, json)
        }
        Command::Suggest { query } => {
            let suggestions = client.search().query(query).suggest().await?;
            render::suggestions(&suggestions, json)
        }
        Command::Product { product } => {
            // A bare number is a product number; anything else is already the slug, which
            // saves the search that turning a number into one costs.
            let details = match product.parse::<ProductId>() {
                Ok(id) => client.products().get(&id).await?,
                Err(_) => client.products().get_by_url(&product).await?,
            };
            render::product(&details, json)
        }
        Command::Basket { action: None } => {
            let basket = client.basket().get().await?;
            render::basket(&basket, json)
        }
        Command::Basket {
            action: Some(BasketAction::Set { product, quantity }),
        } => {
            let id = product_id(&product)?;
            let basket = client.basket().set_quantity(&id, quantity).await?;
            render::basket(&basket, json)
        }
        Command::Basket {
            action: Some(BasketAction::Empty { yes }),
        } => {
            if !yes {
                return Err(CliError::setup(
                    "this removes every line; pass --yes to confirm",
                ));
            }
            let basket = client.basket().empty().await?;
            render::basket(&basket, json)
        }
        Command::Orders { number, skip } => {
            let history = client.orders().history(skip, number).await?;
            render::orders(&history, json)
        }
        Command::Order { id } => {
            let order = client.orders().get(id).await?;
            render::order(&order, json)
        }
        Command::Delivery => {
            let spot = client.delivery().spot().await?;
            render::delivery(spot.as_ref(), json)
        }
        Command::Placements => {
            let placements = client.delivery().placements().await?;
            render::placements(&placements, json)
        }
        Command::Cards => {
            let cards = client.checkout().credit_cards().await?;
            render::cards(&cards, json)
        }
    }
}

fn product_id(raw: &str) -> Result<ProductId> {
    raw.parse()
        .map_err(|error| CliError::setup(format!("{error}")))
}

fn build_client(global: &Global) -> Result<Client> {
    let username = global.username.clone().ok_or_else(|| {
        CliError::setup("no account given; pass --username or set NEMLIG_USERNAME")
    })?;
    let password = read_password(global)?;

    let mut builder = Client::builder()
        .credentials(Credentials::new(username, password))
        .timeout(Duration::from_secs(global.timeout));

    if let Some(base) = &global.base_url {
        builder = builder.base_url(base.clone());
    }
    if let Some(base) = &global.search_base_url {
        builder = builder.search_base_url(base.clone());
    }
    if let Some(path) = session_file(global)? {
        builder = builder.session_file(path);
    }

    builder.build().map_err(CliError::from)
}

/// Where the password comes from.
///
/// A file or the environment, never an argument. `just cli` goes through secretspec, which
/// puts it in the environment for the one process that needs it.
fn read_password(global: &Global) -> Result<String> {
    if let Some(path) = &global.password_file {
        let raw = std::fs::read_to_string(path).map_err(|error| {
            CliError::setup(format!("could not read {}: {error}", path.display()))
        })?;
        return Ok(raw.trim_end_matches(['\r', '\n']).to_owned());
    }
    match std::env::var("NEMLIG_PASSWORD") {
        Ok(password) if !password.is_empty() => Ok(password),
        _ => Err(CliError::setup(
            "no password given; set NEMLIG_PASSWORD or pass --password-file",
        )),
    }
}

/// Where the session is kept, unless the run asked for none.
fn session_file(global: &Global) -> Result<Option<PathBuf>> {
    if global.no_session_file {
        return Ok(None);
    }
    if let Some(path) = &global.session_file {
        return Ok(Some(path.clone()));
    }
    let dirs = directories::ProjectDirs::from("com", "nemlig", "nemlig").ok_or_else(|| {
        CliError::setup("could not work out a cache directory; pass --session-file")
    })?;
    Ok(Some(dirs.cache_dir().join("session.json")))
}

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_env("NEMLIG_LOG")
        .or_else(|_| tracing_subscriber::EnvFilter::try_from_default_env())
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("nemlig=debug"));

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .with_env_filter(filter)
        .init();
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
        let command = Args::command();
        let found = command
            .get_arguments()
            .any(|arg| arg.get_long() == Some("password"));
        assert!(!found, "--password must not exist");
    }

    #[test]
    fn a_quantity_set_is_parsed_as_an_absolute_number() {
        let args =
            Args::try_parse_from(["nemlig", "basket", "set", "5070417", "0"]).expect("parses");
        match args.command {
            Command::Basket {
                action: Some(BasketAction::Set { product, quantity }),
            } => {
                assert_eq!(product, "5070417");
                assert_eq!(quantity, 0);
            }
            other => panic!("parsed as {other:?}"),
        }
    }

    #[test]
    fn emptying_the_basket_needs_the_confirmation() {
        let args = Args::try_parse_from(["nemlig", "basket", "empty"]).expect("parses");
        match args.command {
            Command::Basket {
                action: Some(BasketAction::Empty { yes }),
            } => assert!(!yes),
            other => panic!("parsed as {other:?}"),
        }
    }

    #[test]
    fn a_session_file_and_no_session_file_cannot_both_be_asked_for() {
        assert!(
            Args::try_parse_from([
                "nemlig",
                "--session-file",
                "/tmp/x",
                "--no-session-file",
                "whoami",
            ])
            .is_err()
        );
    }
}
