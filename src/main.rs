use std::process::ExitCode;

use dinnerbot::config::Config;
use dinnerbot::data::FoodData;
use dinnerbot::store::Store;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    let env_file = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    if let Err(error) = env_file {
        // No .env is fine when the variables come from the environment.
        if !error.not_found() {
            warn!(%error, "could not read .env");
        }
    }

    let config = match Config::from_env() {
        Ok(config) => config,
        Err(error) => {
            error!("{error}");
            return ExitCode::FAILURE;
        }
    };

    let data = match load_data(&config) {
        Ok(data) => data,
        Err(error) => {
            error!("{error}");
            return ExitCode::FAILURE;
        }
    };
    info!(
        foods = data.food_count(),
        categories = data.categories.len(),
        "menu loaded"
    );

    // A broken state file stops the bot rather than being overwritten.
    let store = match Store::open(&config.state_path) {
        Ok(store) => store,
        Err(error) => {
            error!("{error}");
            return ExitCode::FAILURE;
        }
    };

    match dinnerbot::discord::run(config, data, store).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            error!("bot stopped: {error}");
            ExitCode::FAILURE
        }
    }
}

fn load_data(config: &Config) -> Result<FoodData, String> {
    let Some(path) = &config.data_path else {
        return FoodData::embedded().map_err(|error| format!("built-in menu is invalid: {error}"));
    };
    let json = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    FoodData::from_json(&json).map_err(|error| format!("{}: {error}", path.display()))
}
