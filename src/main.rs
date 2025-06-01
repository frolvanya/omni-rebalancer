use anyhow::{Context, Result};
use clap::Parser;

mod config;
mod utils;

#[derive(Parser)]
struct CliArgs {
    /// Path to the configuration file
    #[clap(short, long, default_value = "config.toml")]
    config: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();

    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .context("Failed to set global subscriber")?;

    let args = CliArgs::parse();

    let config = toml::from_str::<config::Config>(
        &std::fs::read_to_string(args.config).context("Config file doesn't exist")?,
    )
    .context("Failed to parse config file")?;

    let client = utils::Client::build(config.clone())?;

    let mut handles = Vec::new();

    handles.extend(client.balance_watcher().await);

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Received Ctrl+C signal, shutting down.");
        }
        result = futures::future::select_all(handles) => {
            let (res, _, _) = result;
            if let Ok(Err(err)) = res {
                tracing::error!("A worker encountered an error: {err:?}");
            }
        }
    }

    Ok(())
}
