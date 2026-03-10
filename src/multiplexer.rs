use serde::{Deserialize, Serialize};

/// A terminal multiplexer that can manage sessions inside a container.
pub trait Multiplexer {
    /// Human-readable name (e.g. "zellij", "tmux").
    fn name(&self) -> &str;

    /// Shell command to install this multiplexer inside the container.
    fn install_cmd(&self) -> &str;

    /// Shell command to create or attach to a named session.
    fn attach_cmd(&self, session: &str) -> String;
}

/// Configured multiplexer choice, deserializable from config TOML.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MultiplexerKind {
    Zellij,
    Tmux,
}

impl MultiplexerKind {
    pub fn get(&self) -> Box<dyn Multiplexer> {
        match self {
            MultiplexerKind::Zellij => Box::new(Zellij),
            MultiplexerKind::Tmux => Box::new(Tmux),
        }
    }
}

struct Zellij;

impl Multiplexer for Zellij {
    fn name(&self) -> &str {
        "zellij"
    }

    fn install_cmd(&self) -> &str {
        "cargo-binstall -y --install-path /usr/local/bin zellij"
    }

    fn attach_cmd(&self, session: &str) -> String {
        format!("zellij attach --create {session}")
    }
}

struct Tmux;

impl Multiplexer for Tmux {
    fn name(&self) -> &str {
        "tmux"
    }

    fn install_cmd(&self) -> &str {
        "apt-get install -y tmux"
    }

    fn attach_cmd(&self, session: &str) -> String {
        format!("tmux new-session -A -s {session}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zellij_attach_cmd() {
        let mux = Zellij;
        assert_eq!(
            mux.attach_cmd("my-shade"),
            "zellij attach --create my-shade"
        );
    }

    #[test]
    fn test_tmux_attach_cmd() {
        let mux = Tmux;
        assert_eq!(
            mux.attach_cmd("my-shade"),
            "tmux new-session -A -s my-shade"
        );
    }

    #[test]
    fn test_deserialize_kind() {
        let toml_str = r#"multiplexer = "zellij""#;

        #[derive(Deserialize)]
        struct Config {
            multiplexer: MultiplexerKind,
        }

        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.multiplexer, MultiplexerKind::Zellij);
    }

    #[test]
    fn test_kind_dispatches() {
        let kind = MultiplexerKind::Zellij;
        let mux = kind.get();
        assert_eq!(mux.name(), "zellij");

        let kind = MultiplexerKind::Tmux;
        let mux = kind.get();
        assert_eq!(mux.name(), "tmux");
    }
}
