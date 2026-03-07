use anyhow::Result;
use clap::CommandFactory;

use crate::Cli;

fn generate_completions(shell: clap_complete::Shell) -> String {
    let mut cmd = Cli::command();
    let mut buf = Vec::new();
    clap_complete::generate(shell, &mut cmd, "shade", &mut buf);
    String::from_utf8(buf).expect("completions should be valid utf-8")
}

pub fn shell_init(shell: &str) -> Result<String> {
    let mut output = String::new();

    let completions_shell = match shell {
        "fish" => {
            output.push_str(FISH_FUNCTION);
            clap_complete::Shell::Fish
        }
        "bash" => {
            output.push_str(BASH_FUNCTION);
            clap_complete::Shell::Bash
        }
        "zsh" => {
            output.push_str(ZSH_FUNCTION);
            clap_complete::Shell::Zsh
        }
        _ => anyhow::bail!("unsupported shell: {}. Use fish, bash, or zsh", shell),
    };

    output.push('\n');
    output.push_str(&generate_completions(completions_shell));
    output.push('\n');

    match shell {
        "fish" => output.push_str(FISH_COMPLETIONS),
        "bash" => output.push_str(BASH_COMPLETIONS),
        "zsh" => output.push_str(ZSH_COMPLETIONS),
        _ => {}
    }

    Ok(output)
}

const FISH_FUNCTION: &str = r#"function s --description "Open a shade environment"
    switch "$argv[1]"
        case new cd
            set -l path (command shade $argv | tail -n 1)
            if test -n "$path"
                cd "$path"
            end
        case docker list delete config init help version
            command shade $argv
        case '*'
            set -l path (command shade new $argv | tail -n 1)
            if test -n "$path"
                cd "$path"
            end
    end
end
"#;

const BASH_FUNCTION: &str = r#"s() {
    case "$1" in
        new|cd)
            local path
            path="$(command shade "$@" | tail -n 1)"
            if [ -n "$path" ]; then
                cd "$path" || return
            fi
            ;;
        docker|list|delete|config|init|help|version)
            command shade "$@"
            ;;
        *)
            local path
            path="$(command shade new "$@" | tail -n 1)"
            if [ -n "$path" ]; then
                cd "$path" || return
            fi
            ;;
    esac
}
"#;

const ZSH_FUNCTION: &str = r#"s() {
    case "$1" in
        new|cd)
            local path
            path="$(command shade "$@" | tail -n 1)"
            if [[ -n "$path" ]]; then
                cd "$path" || return
            fi
            ;;
        docker|list|delete|config|init|help|version)
            command shade "$@"
            ;;
        *)
            local path
            path="$(command shade new "$@" | tail -n 1)"
            if [[ -n "$path" ]]; then
                cd "$path" || return
            fi
            ;;
    esac
}
"#;

const FISH_COMPLETIONS: &str = r#"# Dynamic completions for shade names
complete -c shade -n '__fish_seen_subcommand_from cd' -f -a '(command shade list 2>/dev/null)'
complete -c shade -n '__fish_seen_subcommand_from delete' -f -a '(command shade list 2>/dev/null)'
"#;

const BASH_COMPLETIONS: &str = r#"# Dynamic completions for shade names
_shade_complete() {
    local cur prev
    cur="${COMP_WORDS[COMP_CWORD]}"
    prev="${COMP_WORDS[COMP_CWORD-1]}"
    case "$prev" in
        cd|delete)
            COMPREPLY=($(compgen -W "$(command shade list 2>/dev/null)" -- "$cur"))
            ;;
    esac
}
complete -F _shade_complete shade
"#;

const ZSH_COMPLETIONS: &str = r#"# Dynamic completions for shade names
_shade_names() {
    local -a names
    names=(${(f)"$(command shade list 2>/dev/null)"})
    compadd -a names
}
compdef '_arguments "1:command:(new list cd delete docker init config)" "*::arg:->args"' shade
_shade() {
    case "$words[2]" in
        cd|delete) _shade_names ;;
    esac
}
compdef _shade shade
"#;
