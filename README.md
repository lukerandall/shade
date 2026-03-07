# Shade

Ephemeral development environments powered by [Jujutsu](https://github.com/jj-vcs/jj)
workspaces. Quickly create isolated, labelled sandboxes
with linked repos and optional Docker containers for
safe agent-driven development.

## How it works

Each shade is a dated, named directory (e.g. `2026-03-07-my-feature`) under a
configurable root. When you create a shade, you pick which of your repos to link
into it — Shade creates Jujutsu workspaces so each shade gets its own working
copy without cloning. Optionally, you can spin up a Docker container scoped to
the shade with your tools, secrets, and repos mounted in.

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

s keychain set <name> # Store a secret
s keychain get <name> # Retrieve a secret
s keychain list       # List secrets
```

## Configuration

Shade is configured via `~/.config/shade/config.toml`:

```toml
env_dir = "~/Shades"
code_dirs = ["~/Code"]
keychain_prefix = "shade."

[env]
GH_TOKEN = { keychain = "gh-token" }

[docker]
image = "ubuntu:latest"
mounts = ["~/.config:/root/.config"]
setup = "apt-get update && apt-get install -y ripgrep curl"
```

Per-shade overrides can be placed in `shade.toml` inside the shade directory to
customize the Docker image, mounts, or environment for a specific shade.

## Secrets / Keychain

Shade can inject secrets into Docker containers via environment variables. Secrets
can be stored in and retrieved from the system keychain using the `shade keychain`
command, which wraps the platform-specific keychain interface (currently macOS
Keychain is the only backend, but the module is designed with a trait so others
can be added).

### Managing secrets

```bash
# Store a secret (value as argument)
shade keychain set gh-token ghp_abc123

# Store a secret (prompted from stdin)
shade keychain set gh-token

# Retrieve a secret
shade keychain get gh-token

# List all shade-managed secrets
shade keychain list

# Delete a secret
shade keychain delete gh-token
```

A configurable prefix (default `shade.`) is applied to all service names
automatically, so `shade keychain set gh-token` stores the value under the
keychain service `shade.gh-token`. The prefix is set in your config file:

```toml
keychain_prefix = "shade."
```

### Using secrets in environments

Reference keychain entries in your `config.toml` using the short name -- the
prefix is applied automatically:

```toml
[env]
GH_TOKEN = { keychain = "gh-token" }
```

You can also use shell commands or static values:

```toml
[env]
STATIC_VAR = "some-value"
DYNAMIC_VAR = { command = "cat ~/.secrets/token" }
```

## Other tools

- [Scry](https://github.com/stephendolan/scry) — the inspiration for this project. Scry provides ephemeral workspaces for safe AI-assisted development, built around Git worktrees.
