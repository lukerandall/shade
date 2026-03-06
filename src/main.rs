mod config;
mod env;
mod repo_select;
mod slug;
mod tui;
mod vcs;

use anyhow::Result;
use clap::Parser;

use vcs::Vcs;
use vcs::jj::JjVcs;

#[derive(Parser)]
#[command(name = "shade", about = "Ephemeral development environments")]
struct Cli {
    /// Skip the repo selection step
    #[arg(short = 'R', long = "skip-repos")]
    skip_repos: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = config::Config::load()?;
    let vcs = JjVcs;

    match tui::run_tui(&config)? {
        tui::TuiResult::Selected(environment) => {
            println!("{}", environment.path.display());
        }
        tui::TuiResult::Create(label) => {
            let environment = env::create_environment(&config.env_dir, &label)?;

            if !cli.skip_repos {
                let repos = vcs.discover_repos(&config.code_dirs)?;
                if !repos.is_empty() {
                    let current_repo = std::env::current_dir()
                        .ok()
                        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()));

                    match repo_select::run_repo_select(repos, current_repo.as_deref())? {
                        repo_select::RepoSelectResult::Selected(selected) => {
                            for repo in &selected {
                                print!("Creating workspace for {}... ", repo.name);
                                match vcs.create_workspace(repo, &environment.path, &label) {
                                    Ok(()) => println!("done"),
                                    Err(e) => println!("failed: {}", e),
                                }
                            }
                        }
                        repo_select::RepoSelectResult::Cancelled => {}
                    }
                }
            }

            println!("{}", environment.path.display());
        }
        tui::TuiResult::Cancelled => {}
    }

    Ok(())
}
