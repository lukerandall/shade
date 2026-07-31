# Shade

Ephemeral development environments for safe agent-driven
development. Quickly create isolated, labelled sandboxes
with linked repos and optional Docker containers.

## How it works

Each shade is a dated, named directory (e.g. `2026-03-07-my-feature`) under a
configurable root. When you create a shade, Shade scans your `code_dirs` for
repositories and presents an interactive picker so you can choose which repos to
link into the shade. If no `code_dirs` are configured or no repositories are
found, this step is skipped. Shade supports two version control systems:

- **[Jujutsu](https://github.com/jj-vcs/jj)** (default) — links repos via jj workspaces
- **Git** — links repos via git worktrees

In both cases, each shade gets its own working copy without a full clone. Shades
are useful on their own as lightweight, disposable workspaces. For stronger
isolation, you can optionally spin up a Docker container scoped to the shade with
your tools, secrets, and repos mounted in — but Docker is not required.

## Task workflow

Every shade is scaffolded to support a stop-and-resume, agent-driven workflow. On
creation, Shade writes these files into the shade directory (existing ones are
never overwritten):

- **`AGENTS.md`** — describes the environment *and* the ways of working: two tiers
  of agent (an **orchestrator** that drives the task and delegates, and
  **implementers** given specific scoped units of work) plus the file protocol
  below.
- **`TASK.md`** — the high-level brief and north star for the task.
- **`LOG.md`** — an append-only, chronological journal agents write to as they
  work. This is where anyone (or any agent) catches up on what has happened.
- **`DECISIONS.md`** — durable decisions with their rationale and alternatives.
- **`tasks/`** — one file per delegated unit (`tasks/NNN-slug.md`): the brief the
  orchestrator hands an implementer, plus the outcome the implementer records — a
  preserved audit trail of what each implementer was asked to do and did.

Because the state of a task lives entirely in these files, work can be stopped and
resumed at any time — a fresh session reconstructs where things stand from
`TASK.md`, `DECISIONS.md`, and the tail of `LOG.md`. Changes of direction — scope
shifts, abandoned approaches, decisions that change what's being built — are
logged as they happen, so a resumed session never loses them. When a task is
finished, the orchestrator writes a `DONE.md` marker (completion date, summary,
where the work landed) — the signal that a shade is safe to clean up.

These [Claude Code](https://claude.ai/code) skills under `skills/` drive this, and
`bin/install` links them into `~/.claude/skills`:

- **`/shade-plan`** — turn a rough idea into a grounded `TASK.md` (the front door).
- **`/shade-orchestrate`** — start or resume the orchestrator for the current shade.
- **`/shade-implement`** — run as an implementer against one scoped task.
- **`/shade-status`** — summarise where a shade stands; `--html` writes a status page.
- **`/shade-dashboard`** — a numbered status board across all shades; say "deeper N" to
  drill into one. Enriched with live agent state from [herdr](https://herdr.dev)
  when it's running.
- **`/shade-tidy`** — survey accumulated shades and clean up the finished ones.
- **`/shade-graduate`** — promote the current session into a shade when a quick
  exploration has grown into real work: links the repos in play and carries the
  conversation so far into the shade's `TASK.md`/`DECISIONS.md`/`LOG.md`.

### herdr integration

Shade integrates optionally with [herdr](https://herdr.dev), a terminal agent
multiplexer. Enable it in your config:

```toml
[herdr]
enabled = true
```

With this on, creating a shade opens it as a herdr workspace, and the `/shade-status`
and `/shade-orchestrate` skills push a status badge (state, progress, headline) onto
that workspace via `shade herdr report`, so herdr's workspace list reflects where
each task stands. You can also drive it manually:

```bash
shade herdr open                                  # open the current shade as a workspace
shade herdr report --state active --progress 3/7 --headline "..."
```

All herdr interaction is best-effort: if herdr isn't installed or its server isn't
running, shade carries on without it. `/shade-dashboard` also reads live agent state from
herdr when it's running, to show which shades have an agent actively working.

## Quick start

Add shell integration to your shell config (fish shown here):

```fish
shade init fish | source
```

This gives you a wrapper function `s` that handles directory switching
automatically.

### Primary commands

```bash
s                     # Create or select a shade (interactive TUI)
s new                 # Same as above
s cd <name>           # Switch to an existing shade
s delete <name>       # Delete a shade and clean up its workspaces
s list                # List all shades

s docker run          # Start or attach to the shade's Docker container
s docker build        # Pre-build a Docker image with setup baked in
s docker rm           # Remove the shade's Docker container

s config new          # Generate a default config file
s config edit         # Open the config in $EDITOR
s config generate     # Print a default config to stdout
s config path         # Print the config file path

s secret set <name>   # Store a secret
s secret get <name>   # Retrieve a secret
s secret list         # List secrets
```

## Configuration

Shade is configured via `~/.config/shade/config.toml`:

```toml
env_dir = "~/Shades"
code_dirs = ["~/Code"]
secret_prefix = "shade."

# Version control system: "jj" (Jujutsu) or "git".
# vcs = "jj"

# How repos are linked: "workspace" or "clone".
# link_mode = "workspace"

[env]
GH_TOKEN = { secret = "gh-token" }

[docker]
image = "ubuntu:latest"
mounts = ["~/.config:~/.config"]
base_image_setup = "apt-get update && apt-get install -y ripgrep curl"
```

Per-shade overrides can be placed in `shade.toml` inside the shade directory to
customize the Docker image, mounts, or environment for a specific shade.

### Version control

By default, Shade uses **Jujutsu** (jj) workspaces to link repos. Set `vcs = "git"`
to use git worktrees instead. The `link_mode` controls how repos are linked:

- `"workspace"` (default) — shared history, lightweight. Changes in any workspace
  are visible in the primary repo.
- `"clone"` — independent copy, safer for untrusted agents.

## Secrets

Shade can inject secrets into Docker containers via environment variables. Secrets
can be stored in and retrieved from the system keychain using the `shade secret`
command, which wraps the platform-specific keychain interface (currently macOS
Keychain is the only backend, but the module is designed with a trait so others
can be added).

### Managing secrets

```bash
# Store a secret (value as argument)
shade secret set gh-token ghp_abc123

# Store a secret (prompted from stdin)
shade secret set gh-token

# Retrieve a secret
shade secret get gh-token

# List all shade-managed secrets
shade secret list

# Delete a secret
shade secret delete gh-token
```

A configurable prefix (default `shade.`) is applied to all secret names
automatically, so `shade secret set gh-token` stores the value under the
secret name `shade.gh-token`. The prefix is set in your config file:

```toml
secret_prefix = "shade."
```

### Using secrets in environments

Reference secrets in your `config.toml` using the short name -- the
prefix is applied automatically:

```toml
[env]
GH_TOKEN = { secret = "gh-token" }
```

You can also use shell commands or static values:

```toml
[env]
STATIC_VAR = "some-value"
DYNAMIC_VAR = { command = "cat ~/.secrets/token" }
```

### Common secrets

**Claude Code** — generate an OAuth token with `claude setup-token` and store it:

```bash
shade secret set claude sk-ant-o...
```

```toml
[env]
CLAUDE_CODE_OAUTH_TOKEN = { secret = "claude" }
```

**GitHub** — create a [personal access token](https://github.com/settings/tokens)
and store it for use with `gh` and other GitHub tooling:

```bash
shade secret set github ghp_your_token_here
```

```toml
[env]
GH_TOKEN = { secret = "github" }
```

## Other tools

- [Scry](https://github.com/stephendolan/scry) — the inspiration for this project. Scry provides ephemeral workspaces for safe AI-assisted development, built around Git worktrees.
