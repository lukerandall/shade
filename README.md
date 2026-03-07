# Shade

Ephemeral development environments powered by [Jujutsu](https://github.com/jj-vcs/jj)
workspaces. Quickly create isolated, labelled sandboxes
with linked repos and optional Docker containers for
safe agent-driven development.

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
