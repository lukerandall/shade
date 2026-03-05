mod config;
mod env;
mod slug;
mod tui;

use anyhow::Result;

fn main() -> Result<()> {
    let config = config::Config::load()?;

    match tui::run_tui(&config)? {
        tui::TuiResult::Selected(environment) => {
            println!("{}", environment.path.display());
        }
        tui::TuiResult::Create(label) => {
            let environment = env::create_environment(&config.env_dir, &label)?;
            println!("{}", environment.path.display());
        }
        tui::TuiResult::Cancelled => {
            // Exit silently
        }
    }

    Ok(())
}
