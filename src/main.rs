mod config;
mod container;
mod credentials;
mod docker;
mod env;
mod env_vars;
mod keychain;
mod multiplexer;
mod repo_select;
mod shade_config;
mod shell_init;
mod slug;
mod tui;
mod vcs;

use anyhow::{Context, Result};
use clap::Parser;

use keychain::SecretStore;
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
enum ConfigCommand {
    /// Generate a default configuration file
    New,
    /// Open the configuration file in $EDITOR
    Edit,
}

#[derive(clap::Subcommand)]
enum KeychainCommand {
    /// Store a secret in the keychain
    Set {
        /// Service name (prefix from config is applied automatically)
        name: String,
        /// Secret value (omit to read from stdin)
        value: Option<String>,
    },
    /// Fetch a secret from the keychain
    Get {
        /// Service name (prefix from config is applied automatically)
        name: String,
    },
    /// List shade-managed keychain entries
    List,
    /// Delete a secret from the keychain
    Delete {
        /// Service name (prefix from config is applied automatically)
        name: String,
    },
}

#[derive(clap::Subcommand)]
enum DockerCommand {
    /// Start or attach to a Docker container for the current shade
    Run,
    /// Pre-build a Docker image with setup already applied
    Build,
    /// Remove the Docker container for the current shade
    Rm,
    /// Remove prebuilt Docker images
    Clean,
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
    /// Switch to a shade environment
    Cd {
        /// Name of the shade (e.g. 2026-03-07-my-feature)
        name: String,
    },
    /// Delete a shade environment
    Delete {
        /// Name of the shade to delete (e.g. 2026-03-07-my-feature)
        name: String,
    },
    /// Start or attach to the Docker container for the current shade
    Run,
    /// Manage Docker containers for shade environments
    #[command(subcommand)]
    Docker(DockerCommand),

    // -- Setup commands --
    /// Output shell integration for your shell
    Init {
        /// Shell to generate integration for
        shell: shell_init::ShellKind,
    },
    /// Manage the shade configuration file
    #[command(subcommand)]
    Config(ConfigCommand),
    /// Manage secrets in the system keychain
    #[command(subcommand)]
    Keychain(KeychainCommand),
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

    let existing = env::list_workspace_dirs(env_path);
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
    // Clean up jj workspaces
    let workspace_names = env::list_workspace_dirs(&environment.path);
    if !workspace_names.is_empty() {
        let repos = vcs.discover_repos(&config.code_dirs).unwrap_or_default();
        for ws_name in &workspace_names {
            if let Some(repo) = repos.iter().find(|r| &r.name == ws_name)
                && let Err(e) = vcs.remove_workspace(repo, &environment.label)
            {
                eprintln!(
                    "warning: failed to remove jj workspace for {ws_name}: {e}\n\
                     You may need to run `jj workspace forget {}` manually.",
                    environment.label
                );
            }
        }
    }

    // Clean up docker container
    docker::remove_container(&environment.name)?;

    // Remove the shade directory
    env::delete_environment(environment)?;
    Ok(())
}

/// Find the shade root directory by walking up from cwd.
fn current_shade_path(env_dir: &str) -> Result<std::path::PathBuf> {
    let cwd = std::env::current_dir().context("could not determine current directory")?;
    let env_dir = std::path::Path::new(env_dir)
        .canonicalize()
        .unwrap_or_else(|_| std::path::PathBuf::from(env_dir));

    let mut candidate = Some(cwd.as_path());
    loop {
        match candidate {
            Some(path) if path.parent() == Some(&env_dir) => return Ok(path.to_path_buf()),
            Some(path) => candidate = path.parent(),
            None => anyhow::bail!(
                "not inside a shade environment (expected to be under {})",
                env_dir.display()
            ),
        }
    }
}

fn run_docker_for_current_shade(config: &config::Config) -> Result<()> {
    let shade_path = current_shade_path(&config.env_dir)?;
    let shade_name = shade_path
        .file_name()
        .context("invalid shade path")?
        .to_string_lossy();

    docker::run_docker(
        &shade_name,
        &shade_path,
        &config.docker,
        &config.env,
        &config.keychain_prefix,
    )
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

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Init { shell } => {
            print!("{}", shell_init::shell_init(shell));
        }
        Command::Config(ConfigCommand::New) => {
            let path = generate_config()?;
            println!("Created config file: {}", path.display());
        }
        Command::Config(ConfigCommand::Edit) => {
            let path = config::Config::default_path();
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
            let status = std::process::Command::new(&editor)
                .arg(&path)
                .status()
                .with_context(|| format!("failed to launch editor: {editor}"))?;
            if !status.success() {
                anyhow::bail!("editor exited with {status}");
            }
        }
        Command::Keychain(ref cmd) => {
            let config = config::Config::load()?;
            let store = keychain::default_store();
            let prefix = &config.keychain_prefix;
            match cmd {
                KeychainCommand::Set { name, value } => {
                    let service = format!("{prefix}{name}");
                    let secret = match value {
                        Some(v) => v.clone(),
                        None => rpassword::prompt_password(format!("Enter value for {name}: "))
                            .context("failed to read secret")?,
                    };
                    store.set(&service, &secret)?;
                    println!("Stored {service}");
                }
                KeychainCommand::Get { name } => {
                    let service = format!("{prefix}{name}");
                    let value = store.get(&service)?;
                    println!("{value}");
                }
                KeychainCommand::List => {
                    let entries = store.list(prefix)?;
                    if entries.is_empty() {
                        println!("No keychain entries with prefix \"{prefix}\"");
                    } else {
                        for entry in &entries {
                            if let Some(short) = entry.strip_prefix(prefix) {
                                println!("{short}");
                            } else {
                                println!("{entry}");
                            }
                        }
                    }
                }
                KeychainCommand::Delete { name } => {
                    let service = format!("{prefix}{name}");
                    store.delete(&service)?;
                    println!("Deleted {service}");
                }
            }
        }
        Command::List => {
            let config = config::Config::load()?;
            let environments = env::list_environments(&config.env_dir)?;
            if environments.is_empty() {
                println!("No shade environments found in {}", config.env_dir);
            } else {
                for environment in &environments {
                    println!("{}", environment.name);
                }
            }
        }
        Command::Cd { ref name } => {
            let config = config::Config::load()?;
            let environments = env::list_environments(&config.env_dir)?;
            let environment = environments
                .iter()
                .find(|e| e.name == *name)
                .with_context(|| format!("shade not found: {name}"))?;
            println!("{}", environment.path.display());
        }
        Command::Delete { ref name } => {
            let config = config::Config::load()?;
            let environments = env::list_environments(&config.env_dir)?;
            let environment = environments
                .iter()
                .find(|e| e.name == *name)
                .with_context(|| format!("shade not found: {name}"))?;
            let vcs = JjVcs;
            delete_shade(environment, &vcs, &config)?;
            println!("Deleted {name}");
        }
        Command::Run | Command::Docker(DockerCommand::Run) => {
            let config = config::Config::load()?;
            run_docker_for_current_shade(&config)?;
        }
        Command::Docker(DockerCommand::Build) => {
            let config = config::Config::load()?;
            let resolved = env_vars::resolve_env(&config.env, &config.keychain_prefix)?;
            docker::build_image(
                &config.docker.image,
                config.docker.setup.as_deref(),
                config.docker.multiplexer.as_ref(),
                &resolved,
                &config.docker.limits,
            )?;
        }
        Command::Docker(DockerCommand::Clean) => {
            docker::clean_images()?;
        }
        Command::Docker(DockerCommand::Rm) => {
            let config = config::Config::load()?;
            let shade_path = current_shade_path(&config.env_dir)?;
            let shade_name = shade_path
                .file_name()
                .context("invalid shade path")?
                .to_string_lossy();
            docker::remove_container(&shade_name)?;
            println!("Removed container for {shade_name}");
        }
        Command::New { skip_repos, repos } => {
            let config = config::Config::load()?;
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
        }
    }

    Ok(())
}
