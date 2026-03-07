mod config;
mod credentials;
mod docker;
mod env;
mod env_vars;
mod repo_select;
mod shade_config;
mod slug;
mod tui;
mod vcs;

use anyhow::{Context, Result};
use clap::Parser;

use vcs::Vcs;
use vcs::jj::JjVcs;

#[derive(Parser)]
#[command(name = "shade", about = "Ephemeral development environments", version)]
#[command(subcommand_required = true, arg_required_else_help = true)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    // -- Environment commands --
    /// Create or select a shade environment
    #[command(next_help_heading = "Environment Commands")]
    New {
        /// Skip the repo selection step when creating a new shade
        #[arg(short = 'R', long = "skip-repos")]
        skip_repos: bool,

        /// Prompt for repo selection even when selecting an existing shade
        #[arg(short = 'r', long = "repos")]
        repos: bool,
    },
    /// List existing shade environments
    List,
    /// Start or attach to a Docker container for the current shade
    Docker,

    // -- Setup commands --
    /// Output shell integration for your shell (fish, bash, zsh)
    Init {
        /// Shell to generate integration for
        shell: String,
    },
    /// Generate a default configuration file
    Config,
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

fn delete_shade(
    environment: &env::Environment,
    vcs: &impl Vcs,
    config: &config::Config,
) -> Result<()> {
    // Find workspace directories inside the shade and forget them in the source repos
    let workspace_names = detect_existing_workspaces(&environment.path);
    if !workspace_names.is_empty() {
        let repos = vcs.discover_repos(&config.code_dirs).unwrap_or_default();
        for ws_name in &workspace_names {
            if let Some(repo) = repos.iter().find(|r| &r.name == ws_name) {
                // Silently forget — we're inside the TUI, can't print
                let _ = vcs.remove_workspace(repo, &environment.label);
            }
        }
    }

    env::delete_environment(environment)?;
    Ok(())
}

fn run_docker_for_current_shade(config: &config::Config) -> Result<()> {
    let cwd = std::env::current_dir().context("could not determine current directory")?;
    let env_dir = std::path::Path::new(&config.env_dir)
        .canonicalize()
        .unwrap_or_else(|_| std::path::PathBuf::from(&config.env_dir));

    // Walk up from cwd to find a directory that's a direct child of env_dir
    let mut candidate = Some(cwd.as_path());
    let shade_path = loop {
        match candidate {
            Some(path) if path.parent() == Some(&env_dir) => break path.to_path_buf(),
            Some(path) => candidate = path.parent(),
            None => anyhow::bail!(
                "not inside a shade environment (expected to be under {})",
                config.env_dir
            ),
        }
    };

    let shade_name = shade_path
        .file_name()
        .context("invalid shade path")?
        .to_string_lossy();

    docker::run_docker(&shade_name, &shade_path, &config.default_image, &config.env)
}

fn generate_config() -> Result<std::path::PathBuf> {
    let path = config::Config::default_path();

    if path.exists() {
        anyhow::bail!("config file already exists: {}", path.display());
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory: {}", parent.display()))?;
    }

    let contents = config::Config::generate_default();
    std::fs::write(&path, &contents)
        .with_context(|| format!("failed to write config file: {}", path.display()))?;

    Ok(path)
}

fn shell_init(shell: &str) -> Result<String> {
    match shell {
        "fish" => Ok(r#"function s --description "Open a shade environment"
    set -l path (command shade new $argv | tail -n 1)
    if test -n "$path"
        cd "$path"
    end
end
"#
        .to_string()),
        "bash" => Ok(r#"s() {
    local path
    path="$(command shade new "$@" | tail -n 1)"
    if [ -n "$path" ]; then
        cd "$path" || return
    fi
}
"#
        .to_string()),
        "zsh" => Ok(r#"s() {
    local path
    path="$(command shade new "$@" | tail -n 1)"
    if [[ -n "$path" ]]; then
        cd "$path" || return
    fi
}
"#
        .to_string()),
        _ => anyhow::bail!("unsupported shell: {}. Use fish, bash, or zsh", shell),
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Init { ref shell } => {
            print!("{}", shell_init(shell)?);
            return Ok(());
        }
        Command::Config => {
            let path = generate_config()?;
            println!("Created config file: {}", path.display());
            return Ok(());
        }
        _ => {}
    }

    let config = config::Config::load()?;

    match cli.command {
        Command::List => {
            let environments = env::list_environments(&config.env_dir)?;
            if environments.is_empty() {
                println!("No shade environments found in {}", config.env_dir);
            } else {
                for environment in &environments {
                    println!("{}", environment.name);
                }
            }
            Ok(())
        }
        Command::Docker => run_docker_for_current_shade(&config),
        Command::New { skip_repos, repos } => {
            let vcs = JjVcs;
            let delete_handler = |environment: &env::Environment| -> Result<()> {
                delete_shade(environment, &vcs, &config)
            };

            match tui::run_tui(&config, delete_handler)? {
                tui::TuiResult::Selected(environment) => {
                    if repos {
                        select_and_create_workspaces(
                            &vcs,
                            &config,
                            &environment.path,
                            &environment.label,
                        )?;
                    }
                    println!("{}", environment.path.display());
                }
                tui::TuiResult::Create(label) => {
                    let environment = env::create_environment(&config.env_dir, &label)?;
                    let shade_cfg = shade_config::ShadeConfig {
                        setup: config.setup.clone(),
                        env: config.env.clone(),
                        ..Default::default()
                    };
                    shade_cfg.save(&environment.path)?;

                    if !skip_repos {
                        select_and_create_workspaces(&vcs, &config, &environment.path, &label)?;
                    }

                    println!("{}", environment.path.display());
                }
                tui::TuiResult::Cancelled => {}
            }
            Ok(())
        }
        Command::Init { .. } | Command::Config => unreachable!(),
    }
}
