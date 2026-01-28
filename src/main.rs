use clap::{Parser, Subcommand};

mod auth;
mod cli;
mod provider;
mod storage;

#[derive(Parser)]
#[command(name = "tokey", about = "Credential manager for apps without SSO")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List providers and accounts
    List {
        /// Filter by provider name
        provider: Option<String>,
    },

    /// Get credentials (JSON to stdout, auto-refreshes expired creds)
    Get {
        /// Provider name (e.g. slack)
        provider: String,
        /// Account label (uses default if omitted)
        account: Option<String>,
        /// Output a single field value instead of JSON
        #[arg(short, long)]
        field: Option<String>,
    },

    /// Add a new account via browser authentication
    Add {
        /// Provider name (e.g. slack)
        provider: String,
        /// Account label
        #[arg(short, long)]
        label: Option<String>,
    },

    /// Force credential renewal (headless, no browser window)
    Refresh {
        /// Provider name (ignored with --all)
        provider: Option<String>,
        /// Account label (uses default if omitted)
        account: Option<String>,
        /// Refresh all accounts across all providers
        #[arg(long)]
        all: bool,
    },

    /// Delete an account and its credentials
    Remove {
        /// Provider name
        provider: String,
        /// Account label
        account: String,
    },

    /// Credential health overview
    Status {
        /// Filter by provider name
        provider: Option<String>,
        /// Filter by account label
        account: Option<String>,
    },

    /// Set the default account for a provider
    Default {
        /// Provider name
        provider: String,
        /// Account label to set as default
        account: String,
    },

    /// Manage the background refresh daemon (macOS launchd)
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
}

#[derive(Subcommand)]
enum DaemonAction {
    /// Install the launchd agent for periodic credential refresh
    Install {
        /// Refresh interval in hours (default: 12)
        #[arg(long, default_value = "12")]
        interval: u64,
    },
    /// Uninstall the launchd agent
    Uninstall,
    /// Check daemon status
    Status,
}

fn main() {
    let cli = Cli::parse();

    let result = match &cli.command {
        Commands::List { provider } => cli::commands::cmd_list(provider.as_deref()),
        Commands::Get {
            provider,
            account,
            field,
        } => cli::commands::cmd_get(provider, account.as_deref(), field.as_deref()),
        Commands::Add { provider, label } => cli::commands::cmd_add(provider, label.as_deref()),
        Commands::Refresh {
            provider,
            account,
            all,
        } => {
            if *all {
                cli::commands::cmd_refresh_all()
            } else {
                let prov = provider.as_deref().unwrap_or_else(|| {
                    eprintln!("error: provider name required (or use --all)");
                    std::process::exit(1);
                });
                cli::commands::cmd_refresh(prov, account.as_deref())
            }
        }
        Commands::Remove { provider, account } => cli::commands::cmd_remove(provider, account),
        Commands::Status { provider, account } => {
            cli::commands::cmd_status(provider.as_deref(), account.as_deref())
        }
        Commands::Default { provider, account } => cli::commands::cmd_default(provider, account),
        Commands::Daemon { action } => match action {
            DaemonAction::Install { interval } => cli::commands::cmd_daemon_install(*interval),
            DaemonAction::Uninstall => cli::commands::cmd_daemon_uninstall(),
            DaemonAction::Status => cli::commands::cmd_daemon_status(),
        },
    };

    if let Err(e) = result {
        eprintln!("error: {:#}", e);
        std::process::exit(1);
    }
}
