use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use vatic::config::AppConfig;
use vatic::daemon::run_daemon;
use vatic::run::run_job;

#[derive(Parser)]
#[command(name = "vatic", about = "AI agent framework")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a job by alias
    Run {
        /// The job alias to run
        alias: String,
    },
    /// List available jobs
    List,
    /// Start the daemon
    Daemon,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("vatic=info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Run { alias } => {
            let app = match AppConfig::load() {
                Ok(app) => app,
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            };

            match run_job(&app, &alias).await {
                Ok(result) => print!("{result}"),
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Commands::List => {
            let app = match AppConfig::load() {
                Ok(app) => app,
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            };

            if app.jobs.is_empty() {
                println!("No jobs configured.");
                return;
            }

            for (alias, config) in &app.jobs {
                let name = config.name.as_deref().unwrap_or("-");
                println!("{alias}\t{name}");
            }
        }
        Commands::Daemon => {
            let app = match AppConfig::load() {
                Ok(app) => app,
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            };

            if let Err(e) = run_daemon(&app).await {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
    }
}
