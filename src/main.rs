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
    /// Skip the repo selection step when creating a new shade
    #[arg(short = 'R', long = "skip-repos")]
    skip_repos: bool,

    /// Prompt for repo selection even when selecting an existing shade
    #[arg(short = 'r', long = "repos")]
    repos: bool,
}

fn detect_existing_workspaces(env_path: &std::path::Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(env_path) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| e.path().join(".jj").is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect()
}

fn select_and_create_workspaces(
    vcs: &impl Vcs,
    config: &config::Config,
    env_path: &std::path::Path,
    workspace_name: &str,
) -> Result<()> {
    let repos = vcs.discover_repos(&config.code_dirs)?;
    if repos.is_empty() {
        return Ok(());
    }

    let existing = detect_existing_workspaces(env_path);
    let current_repo = std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()));

    match repo_select::run_repo_select(repos, current_repo.as_deref(), &existing)? {
        repo_select::RepoSelectResult::Selected(selected) => {
            for repo in &selected {
                print!("Creating workspace for {}... ", repo.name);
                match vcs.create_workspace(repo, env_path, workspace_name) {
                    Ok(()) => println!("done"),
                    Err(e) => println!("failed: {}", e),
                }
            }
        }
        repo_select::RepoSelectResult::Cancelled => {}
    }
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = config::Config::load()?;
    let vcs = JjVcs;

    match tui::run_tui(&config)? {
        tui::TuiResult::Selected(environment) => {
            if cli.repos {
                select_and_create_workspaces(&vcs, &config, &environment.path, &environment.label)?;
            }
            println!("{}", environment.path.display());
        }
        tui::TuiResult::Create(label) => {
            let environment = env::create_environment(&config.env_dir, &label)?;

            if !cli.skip_repos {
                select_and_create_workspaces(&vcs, &config, &environment.path, &label)?;
            }

            println!("{}", environment.path.display());
        }
        tui::TuiResult::Cancelled => {}
    }

    Ok(())
}
