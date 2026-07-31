mod config;
mod container;
mod credentials;
mod docker;
mod env;
mod env_vars;
mod herdr;
mod multiplexer;
mod repo_select;
mod secret;
mod shade_config;
mod shell_init;
mod slug;
mod tui;
mod vcs;

use std::path::Path;

use anyhow::{Context, Result};
use clap::Parser;

use secret::SecretStore;
use vcs::LinkMode;

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
    /// Print a default config to stdout
    Generate,
    /// Print the config file path
    Path,
}

#[derive(clap::Subcommand)]
enum SecretCommand {
    /// Store a secret
    Set {
        /// Secret name (prefix from config is applied automatically)
        name: String,
        /// Secret value (omit to read from stdin)
        value: Option<String>,
    },
    /// Fetch a secret
    Get {
        /// Secret name (prefix from config is applied automatically)
        name: String,
    },
    /// List stored secrets
    List,
    /// Delete a secret
    Delete {
        /// Secret name (prefix from config is applied automatically)
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
enum HerdrCommand {
    /// Open the current (or named) shade as a herdr workspace
    Open {
        /// Shade name; defaults to the shade containing the current directory
        name: Option<String>,
    },
    /// Push a status badge onto the shade's herdr workspace
    Report {
        /// Overall state, e.g. "active", "blocked", "complete", "planned"
        #[arg(long)]
        state: Option<String>,
        /// Progress fraction of units done, e.g. "3/7"
        #[arg(long)]
        progress: Option<String>,
        /// A short one-line headline shown on the badge
        #[arg(long)]
        headline: Option<String>,
        /// Shade name; defaults to the shade containing the current directory
        #[arg(long)]
        name: Option<String>,
    },
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

        /// Clone repos instead of creating workspaces (independent copies)
        #[arg(short = 'c', long = "clone")]
        clone: bool,

        /// Create non-interactively with this label (skips the TUI). Combine
        /// with --repo to link specific repos.
        #[arg(long)]
        label: Option<String>,

        /// Repo to link into a non-interactively created shade (repeatable).
        /// Only used together with --label. Names match a discovered repo or
        /// its final path component.
        #[arg(long = "repo", value_name = "NAME")]
        repo: Vec<String>,
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
    /// Manage stored secrets
    #[command(subcommand)]
    Secret(SecretCommand),
    /// Integrate the current shade with herdr (https://herdr.dev)
    #[command(subcommand)]
    Herdr(HerdrCommand),
}

/// Link or clone selected repos into the shade directory.
/// Returns the list of linked repos (saved to shade.toml).
fn select_and_link_repos(
    vcs: &dyn vcs::Vcs,
    config: &config::Config,
    env_path: &std::path::Path,
    link_mode: LinkMode,
    label: &str,
) -> Result<Vec<shade_config::LinkedRepo>> {
    if config.code_dirs.is_empty() {
        return Ok(Vec::new());
    }

    let repos = vcs.discover_repos(&config.code_dirs)?;
    if repos.is_empty() {
        return Ok(Vec::new());
    }

    let existing = vcs::list_repo_dirs(&env_path.join("repos"));
    let current_repo = std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()));

    let linked_repos =
        match repo_select::run_repo_select(repos, current_repo.as_deref(), &existing)? {
            repo_select::RepoSelectResult::Selected(selected) => {
                link_selected_repos(vcs, env_path, link_mode, label, &selected)
            }
            repo_select::RepoSelectResult::Cancelled => Vec::new(),
        };
    Ok(linked_repos)
}

/// Link or clone a specific set of repos into the shade directory, printing
/// progress. Shared by the interactive picker and the non-interactive
/// `--repo` path. Repos that fail to link are reported and skipped.
fn link_selected_repos(
    vcs: &dyn vcs::Vcs,
    env_path: &std::path::Path,
    link_mode: LinkMode,
    label: &str,
    selected: &[vcs::Repo],
) -> Vec<shade_config::LinkedRepo> {
    let mut linked_repos = Vec::new();
    // Repo entries nest under `repos/` so the rest of the shade tree stays
    // committable (see DECISIONS D8). Create it up front; failure to create it
    // is unrecoverable for linking, so report and bail out of the loop.
    let repos_dir = env_path.join("repos");
    if let Err(e) = std::fs::create_dir_all(&repos_dir) {
        println!("failed to create repos directory: {e}");
        return linked_repos;
    }
    for repo in selected {
        // Use just the final path component as the link name so that
        // grouped repos like "group/repo" link as "repo", not "group/repo".
        let link_name = Path::new(&repo.name)
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| repo.name.clone());
        match link_mode {
            LinkMode::Link => {
                // Link mode creates a real, isolated jj workspace / git worktree
                // in the source repo rather than a bare symlink (see D9), so an
                // agent works in isolation instead of editing the source repo.
                print!("Linking {}... ", repo.name);
                let workspace_path = repos_dir.join(&link_name);
                match vcs.add_workspace(repo, &workspace_path, label) {
                    Ok(()) => {
                        println!("done");
                        linked_repos.push(shade_config::LinkedRepo {
                            name: link_name,
                            primary_repo_path: repo.path.to_string_lossy().to_string(),
                        });
                    }
                    Err(e) => println!("failed: {}", e),
                }
            }
            LinkMode::Clone => {
                print!("Cloning {}... ", repo.name);
                let clone_repo = vcs::Repo {
                    name: link_name.clone(),
                    path: repo.path.clone(),
                };
                match vcs.clone_repo(&clone_repo, &repos_dir) {
                    Ok(()) => {
                        println!("done");
                        linked_repos.push(shade_config::LinkedRepo {
                            name: link_name,
                            primary_repo_path: repo.path.to_string_lossy().to_string(),
                        });
                    }
                    Err(e) => println!("failed: {}", e),
                }
            }
        }
    }
    linked_repos
}

/// Resolve requested repo names against the repos discovered in `code_dirs`.
/// A name matches either the full discovered name (e.g. `group/repo`) or its
/// final path component (`repo`). Errors if any requested name isn't found.
fn resolve_repos_by_name(
    vcs: &dyn vcs::Vcs,
    config: &config::Config,
    names: &[String],
) -> Result<Vec<vcs::Repo>> {
    let discovered = vcs.discover_repos(&config.code_dirs)?;
    let mut resolved = Vec::new();
    for name in names {
        let found = discovered
            .iter()
            .find(|r| {
                r.name == *name
                    || Path::new(&r.name).file_name().map(|f| f.to_string_lossy())
                        == Some(std::borrow::Cow::Borrowed(name.as_str()))
            })
            .with_context(|| {
                format!("repo not found in code_dirs: {name} (run `shade` to see available repos)")
            })?;
        resolved.push(found.clone());
    }
    Ok(resolved)
}

/// Save shade.toml, scaffold the agent docs, and (if enabled) open the shade in
/// herdr. Shared by the interactive and non-interactive creation paths.
fn finalize_new_shade(
    config: &config::Config,
    vcs: &dyn vcs::Vcs,
    environment: &env::Environment,
    linked_repos: Vec<shade_config::LinkedRepo>,
    link_mode: LinkMode,
) -> Result<()> {
    let shade_cfg = shade_config::ShadeConfig {
        env: config.env.clone(),
        vcs: config.vcs_kind,
        link_mode,
        label: if linked_repos.is_empty() {
            None
        } else {
            Some(environment.label.clone())
        },
        shade_setup: config.default_shade_setup.clone(),
        repos: linked_repos.clone(),
        ..Default::default()
    };
    shade_cfg.save(&environment.path)?;

    let repo_names: Vec<String> = vcs::list_repo_dirs(&environment.path.join("repos"));
    write_agent_docs(
        &environment.path,
        &repo_names,
        &linked_repos,
        vcs.name(),
        link_mode,
    )?;

    if config.herdr.enabled {
        open_shade_in_herdr(&environment.name, &environment.path);
    }
    Ok(())
}

/// How-we-work section appended to every generated AGENTS.md. Generic and
/// idempotent: AGENTS.md is regenerated on each call, so this static block is
/// simply re-appended. Describes the two agent tiers and the file protocol that
/// lets a task be stopped and resumed purely from files on disk.
const WAYS_OF_WORKING: &str = r#"
## Ways of Working

Work on a shade is driven by two tiers of agent.

- **Orchestrator** — drives the task to completion. Reads `TASK.md`, plans, breaks
  the work into implementer-sized units, delegates them, and keeps `LOG.md` and
  `DECISIONS.md` current. It coordinates rather than doing the bulk of the
  implementation itself.
- **Implementer** — given one specific, scoped task. Executes it in the relevant
  workspace, logs progress, and commits. It does not redefine the overall task.

### Files

- `TASK.md` — the high-level brief and north star. Read it first to orient. It
  changes rarely.
- `LOG.md` — an append-only, chronological journal. Every agent appends an entry
  when it starts, on notable progress, and when it finishes. Never rewrite
  history; only append.
- `DECISIONS.md` — durable decisions with their rationale and the alternatives
  considered. Append a new entry whenever a non-trivial choice is made.
- `tasks/` — one file per delegated unit of work (`tasks/NNN-slug.md`). The
  orchestrator writes the brief here before dispatching an implementer; the
  implementer records the outcome in the same file. This is the durable,
  auditable record of exactly what each implementer was asked to do and did.

### Delegated units (`tasks/`)

When the orchestrator delegates a unit, it writes the brief to
`tasks/NNN-slug.md` (next zero-padded ordinal) rather than passing it only in
chat, then points the implementer at that file. The implementer reads its brief
there, does the work, and appends an **Outcome** to the same file. `LOG.md`
remains the chronological index and references task files (`Delegated
tasks/003-...`); the task files hold the brief and result detail. This keeps the
full instruction history preserved even across stops, resumes, and handoffs.

### Capturing changes of direction

The log and decision files only protect you if changes are written down *as they
happen*. Whenever the plan or the task itself shifts mid-flight — a new
constraint, an approach that was abandoned, a change to scope or acceptance
criteria, or any decision that changes *what* is being built — record it
immediately, before continuing the work:

- update `TASK.md` if the brief, scope, or acceptance criteria changed;
- append a `DECISIONS.md` entry with the decision, why, and what it supersedes;
- note the shift in `LOG.md` so the timeline reflects it.

Never leave a course-correction only in your head or in the chat — a resumed
session can see neither. If it changed the direction of the work, it gets logged.

### Resume protocol

Anyone (human or agent) can pick up a shade at any time. On start, read in order:
`AGENTS.md` → `TASK.md` → `DECISIONS.md` → the tail of `LOG.md`, then reconstruct
the current state (what is done, what is in flight, what is next) before acting.

### Completion

When every acceptance criterion in `TASK.md` is met and the work has landed
(merged, pushed, or otherwise handed off), the orchestrator writes a `DONE.md`
marker at the shade root and appends a final `LOG.md` entry. `DONE.md` records
the completion date, a one-line summary, and where the work landed (PR links,
branches, or commits). Its presence is the signal that a shade is finished and
safe to archive or clean up; until it exists, treat the shade as still in flight.

### Log entries

Append entries to `LOG.md` in this shape (get the timestamp with
`date '+%Y-%m-%d %H:%M'`):

```
## 2026-07-31 14:02 — orchestrator
Delegated: add retry to the email-renderer client.
Status: in progress
```

Always write to `LOG.md` before you stop, so a resumed session loses nothing.
"#;

/// Write CLAUDE.md and AGENTS.md into the shade directory so they are visible
/// inside the container at /workspace/.
fn write_agent_docs(
    shade_path: &std::path::Path,
    repo_names: &[String],
    repos: &[shade_config::LinkedRepo],
    vcs_name: &str,
    link_mode: LinkMode,
) -> Result<()> {
    std::fs::write(shade_path.join("CLAUDE.md"), "@AGENTS.md\n")?;

    let has_repos = !repos.is_empty();
    // Per-mode phrasing for a repo entry under `repos/`. The generated docs
    // describe the real on-disk *host* layout (see D10) — Link creates an
    // isolated workspace/worktree, Clone an independent clone.
    let entry_kind = match link_mode {
        LinkMode::Link => format!("isolated {vcs_name} workspace/worktree"),
        LinkMode::Clone => "independent clone".to_string(),
    };
    let mut doc = String::from("# Shade Environment\n\n");

    doc.push_str("## Directory Layout\n\n");
    doc.push_str(
        "This directory *is* the shade. Its durable state — `TASK.md`, `LOG.md`, \
         `DECISIONS.md`, `tasks/` — lives here and is committable; the repo entries \
         under `repos/` are git-ignored.\n\n",
    );
    if has_repos {
        doc.push_str(&format!(
            "- `repos/<name>` — each linked repo is an {entry_kind} of the source repo. \
             Work, commit, and branch here.\n"
        ));
        match link_mode {
            LinkMode::Link => doc.push_str(
                "  Each is isolated from the source repo's working copy, so commits do \
                 not touch the source directly.\n",
            ),
            LinkMode::Clone => {
                doc.push_str("  Each is a self-contained clone, independent of the source repo.\n")
            }
        }
    } else {
        doc.push_str("- No repos are linked into this shade yet.\n");
    }
    doc.push_str(
        "\n> Inside `shade run` (container): the shade is mounted at `/workspace/` and \
         each repo appears at `/repos/<name>`. Work under `/repos/<name>` there.\n",
    );

    if !repo_names.is_empty() {
        doc.push_str("\n## Repos\n\n");
        for name in repo_names {
            doc.push_str(&format!("- `{name}` — {entry_kind} at `repos/{name}`\n"));
        }
    }

    doc.push_str("\n## Tools\n\n");
    doc.push_str(&format!("- **Version control**: {vcs_name}\n"));
    if has_repos {
        match link_mode {
            LinkMode::Link => doc.push_str(&format!(
                "- Repo entries are {vcs_name} workspaces/worktrees under `repos/<name>` — \
                 commit, branch, and push from there; the source repo stays untouched\n",
            )),
            LinkMode::Clone => doc.push_str(
                "- Repo entries under `repos/<name>` are independent clones — \
                 commit, branch, and push from there\n",
            ),
        }
    }

    doc.push_str(WAYS_OF_WORKING);

    std::fs::write(shade_path.join("AGENTS.md"), doc)?;

    scaffold_task_docs(shade_path)?;
    Ok(())
}

/// Create the task-tracking documents (`TASK.md`, `LOG.md`, `DECISIONS.md`, and
/// the `tasks/` directory) in the shade directory from built-in templates.
/// Existing files are never overwritten, since they accrue content as the task
/// progresses.
fn scaffold_task_docs(shade_path: &Path) -> Result<()> {
    const TASK_TEMPLATE: &str = r#"# Task

<!-- The high-level brief. This is the north star the orchestrator drives
     towards. Keep it grounded and concrete; it should change rarely. -->

## Goal

_What are we trying to achieve, and why?_

## Scope / workspaces

_Which repos/workspaces does this touch? What is in play?_

## Acceptance criteria

_How do we know the task is done? List concrete, checkable outcomes._

## Out of scope

_What we are deliberately not doing._
"#;

    const LOG_TEMPLATE: &str = r#"# Log

Append-only, chronological journal. Every agent appends an entry when it starts,
on notable progress, and when it finishes — never rewrite earlier entries. This
is the file to read to catch up on what has happened. Timestamp entries with
`date '+%Y-%m-%d %H:%M'`.

Format:

```
## <YYYY-MM-DD HH:MM> — <orchestrator|implementer>
<what happened / what is being done>
Status: <in progress | done | blocked>
```
"#;

    const DECISIONS_TEMPLATE: &str = r#"# Decisions

Durable decisions and their rationale. Append a new entry whenever a non-trivial
choice is made, so future sessions understand why things are the way they are.

## D1: <title>

**Decision:** _what was decided_
**Why:** _the reasoning_
**Alternatives considered:** _what else was weighed, and why it was not chosen_
"#;

    const TASKS_README_TEMPLATE: &str = r#"# Delegated units

One file per unit of work delegated to an implementer, named `NNN-slug.md` with a
zero-padded ordinal (e.g. `001-add-retry-to-email-client.md`).

The orchestrator writes the brief here *before* dispatching an implementer, then
points the implementer at the file. The implementer records what it did in the
same file's **Outcome** section. Together with `../LOG.md` (the chronological
index that references these files) this preserves exactly what each implementer
was asked to do and did — an audit trail that survives stops, resumes, and
handoffs.

Template for a unit file:

```
# Task NNN: <title>

**Status:** proposed | in progress | done | blocked
**Workspace:** <repo/workspace>
**Delegated:** <YYYY-MM-DD HH:MM> — orchestrator

## Brief
<the one scoped thing to do>

## Pointers
<relevant files / decisions / prior log entries to read first>

## Acceptance
<concrete, checkable outcomes>

## Outcome
<implementer: what changed, where (commits), and any follow-ups>
```
"#;

    for (name, template) in [
        ("TASK.md", TASK_TEMPLATE),
        ("LOG.md", LOG_TEMPLATE),
        ("DECISIONS.md", DECISIONS_TEMPLATE),
    ] {
        let path = shade_path.join(name);
        if !path.exists() {
            std::fs::write(&path, template)?;
        }
    }

    let tasks_readme = shade_path.join("tasks").join("README.md");
    if !tasks_readme.exists() {
        std::fs::create_dir_all(tasks_readme.parent().unwrap())?;
        std::fs::write(&tasks_readme, TASKS_README_TEMPLATE)?;
    }

    // Ignore the linked/cloned repo entries so the shade's durable state
    // (TASK.md/LOG.md/DECISIONS.md/tasks/) is committable to SCM (see D8).
    // Never clobber an existing .gitignore.
    const GITIGNORE_TEMPLATE: &str = "/repos/\n";
    let gitignore = shade_path.join(".gitignore");
    if !gitignore.exists() {
        std::fs::write(&gitignore, GITIGNORE_TEMPLATE)?;
    }
    Ok(())
}

/// Tear down the host workspaces a link-mode shade registered in its source
/// repos (see D9). Only link mode registers anything in source repos — clones
/// are self-contained subdirectories of the shade — so this no-ops otherwise.
/// Every removal is best-effort: a per-repo failure (e.g. the source repo was
/// moved or the workspace is already gone) is warned and skipped so teardown
/// never blocks deleting the shade.
fn teardown_host_workspaces(cfg: &shade_config::ShadeConfig, shade_path: &Path, label: &str) {
    if cfg.link_mode != LinkMode::Link {
        return;
    }
    let vcs = vcs::create_vcs(cfg.vcs);
    let repos_dir = shade_path.join("repos");
    for repo in &cfg.repos {
        let workspace_path = repos_dir.join(&repo.name);
        if let Err(e) =
            vcs.remove_workspace(Path::new(&repo.primary_repo_path), label, &workspace_path)
        {
            eprintln!(
                "warning: could not tear down host workspace for {}: {e}",
                repo.name
            );
        }
    }
}

fn delete_shade(environment: &env::Environment) -> Result<()> {
    docker::remove_container(&environment.name)?;
    // Best-effort: close the shade's herdr workspace if one is open. A herdr
    // problem should never block deleting the shade.
    if let Err(e) = herdr::close_workspace_by_label(&environment.name) {
        eprintln!("warning: could not close herdr workspace: {e}");
    }
    // Best-effort: forget/remove any host workspaces this shade registered in
    // its source repos, before the shade directory is removed. Skipped for
    // clones and for old flat-layout shades (no `repos/` entries to match).
    if let Ok(cfg) = shade_config::ShadeConfig::load(&environment.path) {
        let label = cfg
            .label
            .clone()
            .unwrap_or_else(|| environment.label.clone());
        teardown_host_workspaces(&cfg, &environment.path, &label);
    }
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

    let vcs = vcs::create_vcs(config.vcs_kind);

    docker::run_docker(
        &shade_name,
        &shade_path,
        &config.docker,
        &config.env,
        &config.secret_prefix,
        vcs.as_ref(),
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
        Command::Config(ConfigCommand::Generate) => {
            print!("{}", config::Config::generate_default());
        }
        Command::Config(ConfigCommand::Path) => {
            println!("{}", config::Config::default_path().display());
        }
        Command::Secret(ref cmd) => {
            let config = config::Config::load()?;
            let store = secret::default_store();
            let prefix = &config.secret_prefix;
            match cmd {
                SecretCommand::Set { name, value } => {
                    let full_name = format!("{prefix}{name}");
                    let secret = match value {
                        Some(v) => v.clone(),
                        None => rpassword::prompt_password(format!("Enter value for {name}: "))
                            .context("failed to read secret")?,
                    };
                    store.set(&full_name, &secret)?;
                    println!("Stored {full_name}");
                }
                SecretCommand::Get { name } => {
                    let full_name = format!("{prefix}{name}");
                    let value = store.get(&full_name)?;
                    println!("{value}");
                }
                SecretCommand::List => {
                    let entries = store.list(prefix)?;
                    if entries.is_empty() {
                        println!("No secrets with prefix \"{prefix}\"");
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
                SecretCommand::Delete { name } => {
                    let full_name = format!("{prefix}{name}");
                    store.delete(&full_name)?;
                    println!("Deleted {full_name}");
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
            delete_shade(environment)?;
            println!("Deleted {name}");
        }
        Command::Run | Command::Docker(DockerCommand::Run) => {
            let config = config::Config::load()?;
            run_docker_for_current_shade(&config)?;
        }
        Command::Docker(DockerCommand::Build) => {
            let config = config::Config::load()?;
            let resolved = env_vars::resolve_env(&config.env, &config.secret_prefix)?;
            let vcs = vcs::create_vcs(config.vcs_kind);
            docker::build_image(&docker::BuildImageOptions {
                base_image: &config.docker.image,
                base_image_setup: config.docker.base_image_setup.as_deref(),
                base_image_user_setup: config.docker.base_image_user_setup.as_deref(),
                multiplexer: config.docker.multiplexer.as_ref(),
                env: &resolved,
                limits: &config.docker.limits,
                vcs: vcs.as_ref(),
                user: config.docker.user.as_deref(),
            })?;
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
        Command::New {
            skip_repos,
            repos,
            clone,
            label,
            repo,
        } => {
            let config = config::Config::load()?;
            let link_mode = if clone {
                LinkMode::Clone
            } else {
                config.link_mode
            };

            let vcs = vcs::create_vcs(config.vcs_kind);

            // Non-interactive creation: `shade new --label <name> [--repo ...]`.
            // Skips the TUI entirely so agents can graduate a session to a shade.
            if let Some(label) = label {
                let environment = env::create_environment(&config.env_dir, &label)?;
                if config.init_repo {
                    vcs.init_repo(&environment.path)?;
                }
                let linked_repos = if repo.is_empty() {
                    Vec::new()
                } else {
                    let selected = resolve_repos_by_name(vcs.as_ref(), &config, &repo)?;
                    link_selected_repos(
                        vcs.as_ref(),
                        &environment.path,
                        link_mode,
                        &label,
                        &selected,
                    )
                };
                finalize_new_shade(&config, vcs.as_ref(), &environment, linked_repos, link_mode)?;
                println!("{}", environment.path.display());
                return Ok(());
            }

            let delete_handler =
                |environment: &env::Environment| -> Result<()> { delete_shade(environment) };

            match tui::run_tui(&config, delete_handler)? {
                tui::TuiResult::Selected(environment) => {
                    if repos {
                        let linked_repos = select_and_link_repos(
                            vcs.as_ref(),
                            &config,
                            &environment.path,
                            link_mode,
                            &environment.label,
                        )?;
                        if !linked_repos.is_empty() {
                            let mut shade_cfg = shade_config::ShadeConfig::load(&environment.path)?;
                            shade_cfg.label = Some(environment.label.clone());
                            shade_cfg.link_mode = link_mode;
                            shade_cfg.repos = linked_repos.clone();
                            shade_cfg.save(&environment.path)?;
                        }
                        let repo_names = vcs::list_repo_dirs(&environment.path.join("repos"));
                        write_agent_docs(
                            &environment.path,
                            &repo_names,
                            &linked_repos,
                            vcs.name(),
                            link_mode,
                        )?;
                    }
                    println!("{}", environment.path.display());
                }
                tui::TuiResult::Create(label) => {
                    let environment = env::create_environment(&config.env_dir, &label)?;

                    if config.init_repo {
                        vcs.init_repo(&environment.path)?;
                    }

                    let mut linked_repos = Vec::new();
                    if !skip_repos {
                        linked_repos = select_and_link_repos(
                            vcs.as_ref(),
                            &config,
                            &environment.path,
                            link_mode,
                            &environment.label,
                        )?;
                    }

                    finalize_new_shade(
                        &config,
                        vcs.as_ref(),
                        &environment,
                        linked_repos,
                        link_mode,
                    )?;

                    println!("{}", environment.path.display());
                }
                tui::TuiResult::Cancelled => {}
            }
        }
        Command::Herdr(HerdrCommand::Open { ref name }) => {
            let config = config::Config::load()?;
            let (shade_name, shade_path) = resolve_shade(&config, name.as_deref())?;
            if !herdr::available() {
                println!("herdr is not running; nothing to do.");
            } else {
                match herdr::open_workspace(&shade_path, &shade_name)? {
                    Some(id) => println!("Opened shade '{shade_name}' as herdr workspace {id}."),
                    None => println!("Opened shade '{shade_name}' in herdr."),
                }
            }
        }
        Command::Herdr(HerdrCommand::Report {
            ref state,
            ref progress,
            ref headline,
            ref name,
        }) => {
            let config = config::Config::load()?;
            let (shade_name, _) = resolve_shade(&config, name.as_deref())?;
            report_shade_to_herdr(
                &shade_name,
                state.as_deref(),
                progress.as_deref(),
                headline.as_deref(),
            )?;
        }
    }

    Ok(())
}

/// Resolve a shade to `(name, path)` from an explicit name or, if none is given,
/// the shade containing the current directory.
fn resolve_shade(
    config: &config::Config,
    name: Option<&str>,
) -> Result<(String, std::path::PathBuf)> {
    match name {
        Some(name) => {
            let environments = env::list_environments(&config.env_dir)?;
            let environment = environments
                .iter()
                .find(|e| e.name == name)
                .with_context(|| format!("shade not found: {name}"))?;
            Ok((environment.name.clone(), environment.path.clone()))
        }
        None => {
            let path = current_shade_path(&config.env_dir)?;
            let name = path
                .file_name()
                .context("invalid shade path")?
                .to_string_lossy()
                .to_string();
            Ok((name, path))
        }
    }
}

/// Open a shade as a herdr workspace, best-effort. Never fails shade creation:
/// herdr problems are reported to stderr and otherwise ignored.
fn open_shade_in_herdr(name: &str, path: &std::path::Path) {
    if !herdr::available() {
        return;
    }
    match herdr::open_workspace(path, name) {
        Ok(_) => {}
        Err(e) => eprintln!("warning: could not open shade in herdr: {e}"),
    }
}

/// Push a status badge onto the shade's herdr workspace, if one exists. No-ops
/// gracefully when herdr isn't running or the shade isn't open as a workspace.
fn report_shade_to_herdr(
    shade_name: &str,
    state: Option<&str>,
    progress: Option<&str>,
    headline: Option<&str>,
) -> Result<()> {
    if !herdr::available() {
        println!("herdr is not running; skipping status report.");
        return Ok(());
    }
    let Some(workspace_id) = herdr::find_workspace_id(shade_name)? else {
        println!("shade '{shade_name}' is not open as a herdr workspace; skipping.");
        return Ok(());
    };

    let mut tokens: Vec<(String, String)> = Vec::new();
    if let Some(state) = state {
        tokens.push(("state".to_string(), state.to_string()));
    }
    if let Some(progress) = progress {
        tokens.push(("progress".to_string(), progress.to_string()));
    }
    if let Some(headline) = headline {
        tokens.push(("headline".to_string(), headline.to_string()));
    }
    if tokens.is_empty() {
        println!("nothing to report (pass --state, --progress, or --headline).");
        return Ok(());
    }

    herdr::report_metadata(&workspace_id, &tokens)?;
    println!("Reported status to herdr workspace {workspace_id}.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as ProcCommand;
    use tempfile::TempDir;

    fn init_jj_repo(path: &Path) {
        std::fs::create_dir_all(path).unwrap();
        let out = ProcCommand::new("jj")
            .args(["git", "init"])
            .current_dir(path)
            .output()
            .unwrap();
        assert!(out.status.success(), "jj git init failed");
    }

    fn init_git_repo_with_commit(path: &Path) {
        std::fs::create_dir_all(path).unwrap();
        let run = |args: &[&str]| {
            let out = ProcCommand::new("git")
                .args(args)
                .current_dir(path)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?} failed");
        };
        run(&["init"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::write(path.join("README.md"), "hello").unwrap();
        run(&["add", "."]);
        run(&["commit", "-m", "initial"]);
    }

    fn jj_workspace_list(source_repo: &Path) -> String {
        let out = ProcCommand::new("jj")
            .args(["workspace", "list"])
            .current_dir(source_repo)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    #[test]
    fn link_mode_creates_jj_workspace_under_repos() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("source");
        init_jj_repo(&source);
        let shade = tmp.path().join("shade");
        std::fs::create_dir_all(&shade).unwrap();

        let vcs = vcs::create_vcs(vcs::VcsKind::Jj);
        let repo = vcs::Repo {
            name: "my-repo".to_string(),
            path: source.clone(),
        };
        let linked = link_selected_repos(vcs.as_ref(), &shade, LinkMode::Link, "my-shade", &[repo]);

        assert_eq!(linked.len(), 1);
        assert_eq!(linked[0].name, "my-repo");
        // Nested under repos/, not at the shade root.
        assert!(shade.join("repos/my-repo/.jj").is_dir());
        assert!(!shade.join("my-repo").exists());
        // Registered as an isolated workspace in the source repo.
        assert!(jj_workspace_list(&source).contains("my-shade"));
    }

    #[test]
    fn link_mode_creates_git_worktree_under_repos() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("source");
        init_git_repo_with_commit(&source);
        let shade = tmp.path().join("shade");
        std::fs::create_dir_all(&shade).unwrap();

        let vcs = vcs::create_vcs(vcs::VcsKind::Git);
        let repo = vcs::Repo {
            name: "my-repo".to_string(),
            path: source.clone(),
        };
        let linked = link_selected_repos(vcs.as_ref(), &shade, LinkMode::Link, "my-shade", &[repo]);

        assert_eq!(linked.len(), 1);
        assert!(shade.join("repos/my-repo/.git").exists());
        assert!(!shade.join("my-repo").exists());
        let worktrees = ProcCommand::new("git")
            .args(["worktree", "list"])
            .current_dir(&source)
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&worktrees.stdout).contains("my-shade"));
    }

    #[test]
    fn clone_mode_creates_clone_under_repos() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("source");
        init_git_repo_with_commit(&source);
        let shade = tmp.path().join("shade");
        std::fs::create_dir_all(&shade).unwrap();

        let vcs = vcs::create_vcs(vcs::VcsKind::Git);
        let repo = vcs::Repo {
            name: "my-repo".to_string(),
            path: source.clone(),
        };
        let linked =
            link_selected_repos(vcs.as_ref(), &shade, LinkMode::Clone, "my-shade", &[repo]);

        assert_eq!(linked.len(), 1);
        // Independent clone nested under repos/, nothing at the shade root.
        assert!(shade.join("repos/my-repo/.git").exists());
        assert!(!shade.join("my-repo").exists());
        // A clone registers no workspace in the source repo.
        let worktrees = ProcCommand::new("git")
            .args(["worktree", "list"])
            .current_dir(&source)
            .output()
            .unwrap();
        assert!(!String::from_utf8_lossy(&worktrees.stdout).contains("my-shade"));
    }

    #[test]
    fn teardown_forgets_link_mode_workspace_in_source() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("source");
        init_jj_repo(&source);
        let shade = tmp.path().join("shade");
        std::fs::create_dir_all(&shade).unwrap();

        let vcs = vcs::create_vcs(vcs::VcsKind::Jj);
        let repo = vcs::Repo {
            name: "my-repo".to_string(),
            path: source.clone(),
        };
        link_selected_repos(vcs.as_ref(), &shade, LinkMode::Link, "my-shade", &[repo]);
        assert!(jj_workspace_list(&source).contains("my-shade"));

        let cfg = shade_config::ShadeConfig {
            vcs: vcs::VcsKind::Jj,
            link_mode: LinkMode::Link,
            label: Some("my-shade".to_string()),
            repos: vec![shade_config::LinkedRepo {
                name: "my-repo".to_string(),
                primary_repo_path: source.to_string_lossy().to_string(),
            }],
            ..Default::default()
        };
        teardown_host_workspaces(&cfg, &shade, "my-shade");

        assert!(
            !jj_workspace_list(&source).contains("my-shade"),
            "workspace should be forgotten in the source repo after teardown"
        );
    }

    #[test]
    fn teardown_skips_clone_mode() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("source");
        init_jj_repo(&source);
        let shade = tmp.path().join("shade");
        std::fs::create_dir_all(shade.join("repos")).unwrap();

        // Register a workspace directly so we can prove teardown leaves it alone
        // when the shade was created in clone mode.
        let vcs = vcs::create_vcs(vcs::VcsKind::Jj);
        let repo = vcs::Repo {
            name: "my-repo".to_string(),
            path: source.clone(),
        };
        vcs.add_workspace(&repo, &shade.join("repos/my-repo"), "my-shade")
            .unwrap();
        assert!(jj_workspace_list(&source).contains("my-shade"));

        let cfg = shade_config::ShadeConfig {
            vcs: vcs::VcsKind::Jj,
            link_mode: LinkMode::Clone,
            label: Some("my-shade".to_string()),
            repos: vec![shade_config::LinkedRepo {
                name: "my-repo".to_string(),
                primary_repo_path: source.to_string_lossy().to_string(),
            }],
            ..Default::default()
        };
        teardown_host_workspaces(&cfg, &shade, "my-shade");

        assert!(
            jj_workspace_list(&source).contains("my-shade"),
            "clone-mode teardown must not touch source repos"
        );
    }

    #[test]
    fn teardown_tolerates_missing_source_repo() {
        let tmp = TempDir::new().unwrap();
        let shade = tmp.path().join("shade");
        std::fs::create_dir_all(&shade).unwrap();

        let cfg = shade_config::ShadeConfig {
            vcs: vcs::VcsKind::Jj,
            link_mode: LinkMode::Link,
            label: Some("my-shade".to_string()),
            repos: vec![shade_config::LinkedRepo {
                name: "my-repo".to_string(),
                primary_repo_path: tmp
                    .path()
                    .join("does-not-exist")
                    .to_string_lossy()
                    .to_string(),
            }],
            ..Default::default()
        };
        // Must not panic when the source repo is gone.
        teardown_host_workspaces(&cfg, &shade, "my-shade");
    }

    #[test]
    fn scaffold_creates_task_docs_with_expected_headers() {
        let tmp = TempDir::new().unwrap();
        scaffold_task_docs(tmp.path()).unwrap();

        let task = std::fs::read_to_string(tmp.path().join("TASK.md")).unwrap();
        assert!(task.starts_with("# Task"));
        assert!(task.contains("## Acceptance criteria"));

        let log = std::fs::read_to_string(tmp.path().join("LOG.md")).unwrap();
        assert!(log.starts_with("# Log"));
        assert!(log.contains("Append-only"));

        let decisions = std::fs::read_to_string(tmp.path().join("DECISIONS.md")).unwrap();
        assert!(decisions.starts_with("# Decisions"));
        assert!(decisions.contains("Alternatives considered"));

        let tasks_readme =
            std::fs::read_to_string(tmp.path().join("tasks").join("README.md")).unwrap();
        assert!(tasks_readme.starts_with("# Delegated units"));
        assert!(tasks_readme.contains("## Outcome"));
    }

    #[test]
    fn scaffold_does_not_overwrite_existing_docs() {
        let tmp = TempDir::new().unwrap();
        let task_path = tmp.path().join("TASK.md");
        std::fs::write(&task_path, "# my real task\ndo not clobber\n").unwrap();
        let gitignore_path = tmp.path().join(".gitignore");
        std::fs::write(&gitignore_path, "/custom/\n").unwrap();

        scaffold_task_docs(tmp.path()).unwrap();

        // Pre-existing files are left untouched...
        let task = std::fs::read_to_string(&task_path).unwrap();
        assert_eq!(task, "# my real task\ndo not clobber\n");
        let gitignore = std::fs::read_to_string(&gitignore_path).unwrap();
        assert_eq!(gitignore, "/custom/\n");
        // ...while the missing ones are still created.
        assert!(tmp.path().join("LOG.md").exists());
        assert!(tmp.path().join("DECISIONS.md").exists());
    }

    #[test]
    fn scaffold_creates_gitignore_ignoring_repos() {
        let tmp = TempDir::new().unwrap();
        scaffold_task_docs(tmp.path()).unwrap();

        let gitignore = std::fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        assert!(
            gitignore.contains("/repos/"),
            "expected .gitignore to ignore /repos/, got: {gitignore}"
        );
    }

    #[test]
    fn agent_docs_include_ways_of_working_and_scaffold() {
        let tmp = TempDir::new().unwrap();
        write_agent_docs(tmp.path(), &[], &[], "jj", LinkMode::Link).unwrap();

        let agents = std::fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap();
        assert!(agents.contains("## Ways of Working"));
        assert!(agents.contains("Orchestrator"));
        assert!(agents.contains("Implementer"));

        // write_agent_docs also lays down the task scaffold (incl. .gitignore).
        assert!(tmp.path().join("TASK.md").exists());
        assert!(tmp.path().join("LOG.md").exists());
        assert!(tmp.path().join("DECISIONS.md").exists());
        assert!(tmp.path().join("tasks").join("README.md").exists());
        assert!(tmp.path().join(".gitignore").exists());
    }

    #[test]
    fn agent_docs_link_mode_describes_isolated_host_workspaces() {
        let tmp = TempDir::new().unwrap();
        let repos = vec![shade_config::LinkedRepo {
            name: "core".to_string(),
            primary_repo_path: "/src/core".to_string(),
        }];
        write_agent_docs(
            tmp.path(),
            &["core".to_string()],
            &repos,
            "jj",
            LinkMode::Link,
        )
        .unwrap();

        let agents = std::fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap();
        // Describes the real host layout under repos/<name>...
        assert!(agents.contains("repos/core"));
        assert!(agents.contains("isolated jj workspace/worktree"));
        // ...and does not make the old unconditional container-only claim in the
        // host-facing sections (it survives only in the labelled container note).
        assert!(!agents.contains("Read-only clones mounted from the host"));
        assert!(!agents.contains("Contains jj workspaces for each repo"));
        // The container story is retained, clearly labelled.
        assert!(agents.contains("Inside `shade run` (container)"));
    }

    #[test]
    fn agent_docs_clone_mode_describes_independent_clones() {
        let tmp = TempDir::new().unwrap();
        let repos = vec![shade_config::LinkedRepo {
            name: "core".to_string(),
            primary_repo_path: "/src/core".to_string(),
        }];
        write_agent_docs(
            tmp.path(),
            &["core".to_string()],
            &repos,
            "git",
            LinkMode::Clone,
        )
        .unwrap();

        let agents = std::fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap();
        assert!(agents.contains("repos/core"));
        assert!(agents.contains("independent clone"));
        // Clone mode must not claim isolated workspaces/worktrees.
        assert!(!agents.contains("workspace/worktree"));
        assert!(agents.contains("Inside `shade run` (container)"));
    }
}
