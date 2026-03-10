//! Docker container management for shade environments.
//!
//! Volume mount architecture:
//! - The shade directory is mounted at `/workspace` inside the container.
//! - Primary repos (source for workspaces) are mounted at `/repos/{name}`.
//! - Workspaces are created inside the container at `/workspace/{name}` so
//!   that VCS internal paths (e.g. `.jj/repo`) use container-local paths.

use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

use crate::container::{ContainerLimits, DockerConfig};
use crate::env_vars::{self, EnvValue};
use crate::multiplexer::MultiplexerKind;
use crate::shade_config::{LinkedRepo, ShadeConfig};
use crate::vcs::Vcs;

/// Check whether a container with the given name exists and its state.
enum ContainerState {
    NotFound,
    Running,
    Stopped,
}

fn container_name(shade_name: &str) -> String {
    format!("shade-{shade_name}")
}

fn inspect_container(name: &str) -> Result<ContainerState> {
    let output = Command::new("docker")
        .args(["inspect", "--format", "{{.State.Running}}", name])
        .output()
        .context("failed to run docker inspect")?;

    if !output.status.success() {
        return Ok(ContainerState::NotFound);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim() == "true" {
        Ok(ContainerState::Running)
    } else {
        Ok(ContainerState::Stopped)
    }
}

/// Expand a leading `~` in a container-side path to the container user's home directory.
fn expand_tilde_container(path: &str, user: Option<&str>) -> String {
    let home = match user {
        Some("root") | None => "/root".to_string(),
        Some(u) => format!("/home/{u}"),
    };
    if path == "~" {
        return home;
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return format!("{home}/{rest}");
    }
    path.to_string()
}

/// Build volume mount arguments for docker run.
///
/// Always mounts the shade directory at `/workspace`. For workspace-mode repos,
/// also mounts each primary repo at `/repos/{name}`.
fn volume_args(shade_path: &Path, workspace_repos: &[LinkedRepo]) -> Vec<String> {
    let mut args = Vec::new();

    // Always mount the shade dir at /workspace
    args.push("-v".to_string());
    args.push(format!("{}:/workspace", shade_path.display()));

    // Mount each primary repo at /repos/{name}
    for repo in workspace_repos {
        args.push("-v".to_string());
        args.push(format!("{}:/repos/{}", repo.primary_repo_path, repo.name));
    }

    args
}

pub fn run_docker(
    shade_name: &str,
    shade_path: &Path,
    root_docker: &DockerConfig,
    root_env: &HashMap<String, EnvValue>,
    keychain_prefix: &str,
    vcs: &dyn Vcs,
) -> Result<()> {
    let name = container_name(shade_name);
    let shade_config = ShadeConfig::load(shade_path)?;
    let docker = root_docker.merge(&shade_config.docker);
    let merged_env = env_vars::merge_env(root_env, &shade_config.env);

    let mux = docker.multiplexer.as_ref();
    let paths = &docker.path;
    let user = docker.user.as_deref();

    match inspect_container(&name)? {
        ContainerState::Running => {
            println!("Attaching to running container {name}...");
            exec_into(&name, shade_name, mux, paths, user)?;
        }
        ContainerState::Stopped => {
            println!("Starting stopped container {name}...");
            start_container(&name, mux)?;
            if mux.is_some() {
                wait_for_ready(&name)?;
                exec_into(&name, shade_name, mux, paths, user)?;
            }
        }
        ContainerState::NotFound => {
            let workspace_repos = &shade_config.workspace_repos;
            let workspace_label = shade_config.label.as_deref().unwrap_or(shade_name);

            let resolved = env_vars::resolve_env(&merged_env, keychain_prefix)?;

            let has_prebuilt = prebuilt_image_exists(
                &docker.image,
                docker.base_image_setup.as_deref(),
                mux,
                vcs.name(),
                user,
            );

            let needs_prebuilt = docker.base_image_setup.is_some()
                || mux.is_some()
                || !workspace_repos.is_empty()
                || user.is_some();
            if !has_prebuilt && needs_prebuilt {
                bail!(
                    "no prebuilt image found. Run `shade docker build` first to bake in setup and tools"
                );
            }

            let effective_image = if has_prebuilt {
                let prebuilt = prebuilt_image_name(
                    &docker.image,
                    docker.base_image_setup.as_deref(),
                    mux,
                    vcs.name(),
                    user,
                );
                println!("Creating container {name} from prebuilt image...");
                prebuilt
            } else {
                println!("Creating container {name} from {}...", docker.image);
                docker.image.clone()
            };

            // Resolve shade_setup: per-shade overrides root default
            let shade_setup = shade_config
                .shade_setup
                .as_deref()
                .or(docker.shade_setup.as_deref());

            create_and_run(&CreateOptions {
                name: &name,
                shade_path,
                workspace_repos,
                workspace_label,
                image: &effective_image,
                env: &resolved,
                mounts: &docker.mounts,
                shade_setup,
                user,
                multiplexer: mux,
                paths,
                limits: &docker.limits,
                detach: mux.is_some(),
                vcs,
            })?;

            if mux.is_some() {
                wait_for_ready(&name)?;
                exec_into(&name, shade_name, mux, paths, user)?;
            }
        }
    }

    Ok(())
}

const SETUP_MARKER: &str = "/tmp/.shade-setup-hash";
const READY_MARKER: &str = "/tmp/.shade-ready";

/// FNV-1a hash — stable across Rust versions and processes, no extra deps.
fn hash_setup(cmd: &str) -> u64 {
    const FNV_OFFSET: u64 = 14695981039346656037;
    const FNV_PRIME: u64 = 1099511628211;
    let mut hash = FNV_OFFSET;
    for byte in cmd.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Build a shell snippet that prepends the configured paths to $PATH.
fn path_export(paths: &[String]) -> Option<String> {
    if paths.is_empty() {
        return None;
    }
    let quoted: Vec<String> = paths
        .iter()
        .map(|p| {
            let escaped = p
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('`', "\\`");
            format!("\"{}\"", escaped)
        })
        .collect();
    let joined = quoted.join(":");
    Some(format!("export PATH={joined}:$PATH"))
}

struct SetupScriptOptions<'a> {
    shade_setup: Option<&'a str>,
    workspace_repos: &'a [LinkedRepo],
    workspace_label: &'a str,
    vcs: &'a dyn Vcs,
    user: Option<&'a str>,
    mux: Option<&'a MultiplexerKind>,
    paths: &'a [String],
    detach: bool,
}

fn setup_script(opts: &SetupScriptOptions) -> String {
    let tail = match (opts.detach, opts.user) {
        (true, Some(u)) => format!("exec runuser -l {u} -c 'sleep infinity'"),
        (true, None) => "exec sleep infinity".to_string(),
        (false, Some(u)) => format!("exec runuser -l {u} -c 'cd /workspace && exec bash'"),
        (false, None) => "exec /bin/bash".to_string(),
    };

    let mux_install = opts.mux.map(|kind| {
        let m = kind.get();
        let cmd = m.install_cmd();
        format!("command -v {} >/dev/null 2>&1 || {{ {cmd}; }}", m.name())
    });

    let mut parts = Vec::new();

    if let Some(export) = path_export(opts.paths) {
        parts.push(export);
    }

    if !opts.workspace_repos.is_empty() {
        // Ensure VCS tool is available before creating workspaces
        let vcs_name = opts.vcs.name();
        parts.push(format!(
            "command -v {vcs_name} >/dev/null 2>&1 || {{ echo 'error: {vcs_name} not found — run shade docker build'; exit 1; }}"
        ));

        // Chown clone and workspace dirs so the configured user can access them
        if let Some(username) = opts.user {
            for r in opts.workspace_repos {
                parts.push(format!("chown -R {username} /repos/{}", r.name));
            }
            parts.push(format!("chown {username} /workspace"));
        }

        for r in opts.workspace_repos {
            let exists_check = opts
                .vcs
                .container_workspace_exists_check(&format!("/workspace/{}", r.name));
            let create_cmd = opts.vcs.container_workspace_cmd(
                &format!("/repos/{}", r.name),
                &format!("/workspace/{}", r.name),
                opts.workspace_label,
            );
            parts.push(format!("if ! {exists_check}; then {create_cmd}; fi"));
        }

        // Chown the created workspace dirs too
        if let Some(username) = opts.user {
            for r in opts.workspace_repos {
                parts.push(format!("chown -R {username} /workspace/{}", r.name));
            }
        }
    }

    if let Some(cmd) = opts.shade_setup {
        let cmd = cmd.trim();
        let hash = hash_setup(cmd);
        let marker = format!("{SETUP_MARKER}-shade");
        let run_cmd = if let Some(username) = opts.user {
            format!("runuser -l {username} -c '{cmd}'")
        } else {
            cmd.to_string()
        };
        parts.push(format!(
            "if [ ! -f {marker} ] || [ \"$(cat {marker})\" != \"{hash}\" ]; then {run_cmd} && echo '{hash}' > {marker}; fi"
        ));
    }

    if let Some(install) = mux_install {
        parts.push(install);
    }

    if opts.detach {
        parts.push(format!("touch {READY_MARKER}"));
    }

    parts.push(tail.to_string());
    parts.join(" && ")
}

struct CreateOptions<'a> {
    name: &'a str,
    shade_path: &'a Path,
    workspace_repos: &'a [LinkedRepo],
    workspace_label: &'a str,
    image: &'a str,
    env: &'a [(String, String)],
    mounts: &'a [String],
    shade_setup: Option<&'a str>,
    user: Option<&'a str>,
    multiplexer: Option<&'a MultiplexerKind>,
    paths: &'a [String],
    limits: &'a ContainerLimits,
    detach: bool,
    vcs: &'a dyn Vcs,
}

fn create_and_run(opts: &CreateOptions) -> Result<()> {
    let mut cmd = Command::new("docker");
    if opts.detach {
        cmd.args(["run", "-d", "--name", opts.name, "-w", "/workspace"]);
    } else {
        cmd.args(["run", "-it", "--name", opts.name, "-w", "/workspace"]);
    }
    cmd.args(opts.limits.docker_args());
    let vol_args = volume_args(opts.shade_path, opts.workspace_repos);
    cmd.args(vol_args);
    for mount in opts.mounts {
        let mount_arg = if let Some((src, tgt)) = mount.split_once(':') {
            format!("{}:{}", src, expand_tilde_container(tgt, opts.user))
        } else {
            format!("{mount}:{mount}")
        };
        cmd.args(["-v", &mount_arg]);
    }
    for (key, value) in opts.env {
        cmd.args(["-e", &format!("{key}={value}")]);
    }
    cmd.arg(opts.image);
    let script = setup_script(&SetupScriptOptions {
        shade_setup: opts.shade_setup,
        workspace_repos: opts.workspace_repos,
        workspace_label: opts.workspace_label,
        vcs: opts.vcs,
        user: opts.user,
        mux: opts.multiplexer,
        paths: opts.paths,
        detach: opts.detach,
    });
    cmd.args(["/bin/bash", "-c", &script]);

    let status = cmd.status().context("failed to run docker")?;
    if !status.success() {
        bail!("docker run exited with {status}");
    }
    Ok(())
}

fn wait_for_ready(name: &str) -> Result<()> {
    use std::io::Write;
    print!("Waiting for container setup to complete...");
    std::io::stdout().flush().ok();
    for _ in 0..600 {
        let output = Command::new("docker")
            .args(["exec", name, "test", "-f", READY_MARKER])
            .output()
            .context("failed to check container readiness")?;
        if output.status.success() {
            println!(" done");
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    bail!("timed out waiting for container setup (10 minutes)")
}

fn start_container(name: &str, mux: Option<&MultiplexerKind>) -> Result<()> {
    let mut cmd = Command::new("docker");
    if mux.is_some() {
        cmd.args(["start", name]);
    } else {
        cmd.args(["start", "-ai", name]);
    }
    let status = cmd.status().context("failed to start container")?;
    if !status.success() {
        bail!("docker start exited with {status}");
    }
    Ok(())
}

fn exec_into(
    name: &str,
    session: &str,
    mux: Option<&MultiplexerKind>,
    paths: &[String],
    user: Option<&str>,
) -> Result<()> {
    let mut cmd = Command::new("docker");
    cmd.args(["exec", "-it"]);
    if let Some(u) = user {
        cmd.args(["-u", u]);
    }
    cmd.arg(name);
    match mux {
        Some(kind) => {
            let m = kind.get();
            let attach = m.attach_cmd(session);
            let mut script_parts = Vec::new();
            if let Some(export) = path_export(paths) {
                script_parts.push(export);
            }
            script_parts.push(attach);
            let script = script_parts.join("; ");
            cmd.args(["/bin/bash", "-c", &script]);
        }
        None => {
            cmd.arg("/bin/bash");
        }
    }
    let status = cmd.status().context("failed to exec into container")?;
    if !status.success() {
        bail!("docker exec exited with {status}");
    }
    Ok(())
}

pub struct BuildImageOptions<'a> {
    pub base_image: &'a str,
    pub base_image_setup: Option<&'a str>,
    pub multiplexer: Option<&'a MultiplexerKind>,
    pub env: &'a [(String, String)],
    pub limits: &'a ContainerLimits,
    pub vcs: &'a dyn Vcs,
    pub user: Option<&'a str>,
}

/// Build a pre-configured image by running setup on the base image and committing.
pub fn build_image(opts: &BuildImageOptions) -> Result<String> {
    let BuildImageOptions {
        base_image,
        base_image_setup,
        multiplexer,
        env,
        limits,
        vcs,
        user,
    } = opts;
    let vcs_name = vcs.name();
    let image_tag =
        prebuilt_image_name(base_image, *base_image_setup, *multiplexer, vcs_name, *user);

    // Check if image already exists
    let check = Command::new("docker")
        .args(["image", "inspect", &image_tag])
        .output()
        .context("failed to run docker image inspect")?;
    if check.status.success() {
        println!("Image {image_tag} already exists. Rebuilding...");
        let _ = Command::new("docker").args(["rmi", &image_tag]).output();
    }

    let temp_name = format!("shade-build-{}", std::process::id());

    let mut cmd = Command::new("docker");
    cmd.args(["run", "--name", &temp_name]);
    let _ = limits; // limits are for runtime containers, not build
    for (key, value) in *env {
        cmd.args(["-e", &format!("{key}={value}")]);
    }
    cmd.arg(*base_image);

    let mut steps = Vec::new();
    if let Some(username) = user {
        println!("Creating user {username} in image...");
        steps.push(format!(
            "id -u {username} >/dev/null 2>&1 || useradd -m -s /bin/bash {username}"
        ));
        // Create VCS config directory with correct ownership
        if vcs_name == "jj" {
            steps.push(format!(
                "mkdir -p /home/{username}/.config/jj && chown -R {username} /home/{username}/.config"
            ));
        }
    }

    // Install cargo-binstall if any tool needs it
    let vcs_install = vcs.install_cmd();
    let needs_binstall = vcs_install.contains("binstall")
        || multiplexer.is_some_and(|k| k.get().install_cmd().contains("binstall"));
    if needs_binstall {
        steps.push(
            "apt-get update -qq && apt-get install -y -qq curl >/dev/null && curl -fsSL https://github.com/cargo-bins/cargo-binstall/raw/main/install-from-binstall-release.sh | bash && mv /root/.cargo/bin/cargo-binstall /usr/local/bin/".to_string(),
        );
    }

    println!("Including {vcs_name} in image...");
    steps.push(vcs_install.to_string());

    if let Some(mux_kind) = multiplexer {
        let mux = mux_kind.get();
        println!("Including {} in image...", mux.name());
        let cmd = mux.install_cmd().replace(
            "cargo-binstall -y",
            "cargo-binstall -y --install-path /usr/local/bin",
        );
        steps.push(cmd);
    }
    if let Some(cmd) = base_image_setup {
        let cmd = cmd.trim();
        let hash = hash_setup(cmd);
        steps.push(format!("{cmd} && echo '{hash}' > {SETUP_MARKER}"));
    }
    if steps.is_empty() {
        steps.push("echo 'No setup command'".to_string());
    }
    let build_script = steps.join(" && ");
    cmd.args(["/bin/bash", "-c", &build_script]);

    println!("Running setup in temporary container...");
    let status = cmd.status().context("failed to run docker")?;
    if !status.success() {
        let _ = Command::new("docker")
            .args(["rm", "-f", &temp_name])
            .output();
        bail!("setup command failed with {status}");
    }

    println!("Committing image as {image_tag}...");
    let output = Command::new("docker")
        .args(["commit", &temp_name, &image_tag])
        .output()
        .context("failed to commit container")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let _ = Command::new("docker")
            .args(["rm", "-f", &temp_name])
            .output();
        bail!("failed to commit image: {stderr}");
    }

    let _ = Command::new("docker").args(["rm", &temp_name]).output();

    println!("Built image {image_tag}");
    Ok(image_tag)
}

pub fn prebuilt_image_exists(
    base_image: &str,
    base_image_setup: Option<&str>,
    multiplexer: Option<&MultiplexerKind>,
    vcs_name: &str,
    user: Option<&str>,
) -> bool {
    let tag = prebuilt_image_name(base_image, base_image_setup, multiplexer, vcs_name, user);
    Command::new("docker")
        .args(["image", "inspect", &tag])
        .output()
        .is_ok_and(|o| o.status.success())
}

fn prebuilt_image_name(
    base_image: &str,
    base_image_setup: Option<&str>,
    multiplexer: Option<&MultiplexerKind>,
    vcs_name: &str,
    user: Option<&str>,
) -> String {
    let mux_str = match multiplexer {
        Some(kind) => kind.get().name().to_string(),
        None => String::new(),
    };
    let user_str = user.unwrap_or("");
    let combined = format!(
        "{base_image}:{}:{mux_str}:{vcs_name}:{user_str}",
        base_image_setup.unwrap_or(""),
    );
    let hash = hash_setup(&combined);
    format!("shade-prebuilt:{hash:x}")
}

/// Remove all prebuilt shade images.
pub fn clean_images() -> Result<()> {
    let output = Command::new("docker")
        .args([
            "images",
            "--format",
            "{{.Repository}}:{{.Tag}}",
            "shade-prebuilt",
        ])
        .output()
        .context("failed to list docker images")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let images: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();

    if images.is_empty() {
        println!("No prebuilt images found");
        return Ok(());
    }

    for image in &images {
        let rm = Command::new("docker")
            .args(["rmi", image])
            .output()
            .context("failed to remove image")?;
        if rm.status.success() {
            println!("Removed {image}");
        } else {
            let stderr = String::from_utf8_lossy(&rm.stderr);
            eprintln!("Failed to remove {image}: {}", stderr.trim());
        }
    }

    Ok(())
}

/// Remove a container if it exists (stopped or running). Silently succeeds if not found.
pub fn remove_container(shade_name: &str) -> Result<()> {
    let name = container_name(shade_name);
    match inspect_container(&name)? {
        ContainerState::NotFound => Ok(()),
        _ => {
            let output = Command::new("docker")
                .args(["rm", "-f", &name])
                .output()
                .context("failed to remove container")?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                bail!("failed to remove container {name}: {stderr}");
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vcs::git::GitVcs;
    use crate::vcs::jj::JjVcs;

    #[test]
    fn test_path_export_allows_variable_expansion() {
        let paths = vec![
            "$HOME/.cargo/bin".to_string(),
            "$HOME/.local/bin".to_string(),
        ];
        let export = path_export(&paths).unwrap();
        assert!(export.contains("$HOME"));
        assert!(export.contains(r#""$HOME/.cargo/bin""#));
    }

    #[test]
    fn test_path_export_escapes_special_chars() {
        let paths = vec![
            r#"/path/with"quotes"#.to_string(),
            r"/path/with\backslash".to_string(),
        ];
        let export = path_export(&paths).unwrap();
        assert!(export.contains(r#"with\"quotes"#));
        assert!(export.contains(r"with\\backslash"));
    }

    #[test]
    fn test_path_export_handles_backticks() {
        let paths = vec!["/path/with`backtick`".to_string()];
        let export = path_export(&paths).unwrap();
        assert!(export.contains(r"with\`backtick\`"));
    }

    #[test]
    fn test_path_export_empty_returns_none() {
        let paths = vec![];
        assert!(path_export(&paths).is_none());
    }

    #[test]
    fn test_path_export_format() {
        let paths = vec!["/usr/local/bin".to_string(), "/opt/bin".to_string()];
        let export = path_export(&paths).unwrap();
        assert_eq!(export, r#"export PATH="/usr/local/bin":"/opt/bin":$PATH"#);
    }

    #[test]
    fn test_volume_args_no_workspace_repos() {
        let shade = Path::new("/home/user/Shades/2026-03-09-test");
        let args = volume_args(shade, &[]);

        assert!(args.contains(&format!("{}:/workspace", shade.display())));
        assert_eq!(args.len(), 2); // -v and the mount
    }

    #[test]
    fn test_volume_args_with_workspace_repos() {
        let shade = Path::new("/home/user/Shades/2026-03-09-test");
        let repos = vec![LinkedRepo {
            name: "core".to_string(),
            primary_repo_path: "/home/user/Code/core".to_string(),
        }];

        let args = volume_args(shade, &repos);

        assert!(args.contains(&format!("{}:/workspace", shade.display())));
        assert!(args.contains(&"/home/user/Code/core:/repos/core".to_string()));
    }

    #[test]
    fn test_volume_args_multiple_workspace_repos() {
        let shade = Path::new("/home/user/Shades/2026-03-09-test");
        let repos = vec![
            LinkedRepo {
                name: "repo-a".to_string(),
                primary_repo_path: "/home/user/Code/repo-a".to_string(),
            },
            LinkedRepo {
                name: "repo-b".to_string(),
                primary_repo_path: "/home/user/Code/repo-b".to_string(),
            },
        ];

        let args = volume_args(shade, &repos);

        // Shade mount should appear exactly once
        let shade_mount = format!("{}:/workspace", shade.display());
        let shade_count = args.iter().filter(|a| **a == shade_mount).count();
        assert_eq!(shade_count, 1);

        assert!(args.contains(&"/home/user/Code/repo-a:/repos/repo-a".to_string()));
        assert!(args.contains(&"/home/user/Code/repo-b:/repos/repo-b".to_string()));
    }

    #[test]
    fn test_setup_script_with_jj_workspace_repos() {
        let vcs = JjVcs;
        let repos = vec![LinkedRepo {
            name: "core".to_string(),
            primary_repo_path: "/home/user/Code/core".to_string(),
        }];
        let script = setup_script(&SetupScriptOptions {
            shade_setup: None,
            workspace_repos: &repos,
            workspace_label: "feat",
            vcs: &vcs,
            user: None,
            mux: None,
            paths: &[],
            detach: false,
        });

        assert!(script.contains("command -v jj"));
        assert!(script.contains("jj workspace add --name feat /workspace/core"));
        assert!(script.ends_with("exec /bin/bash"));
    }

    #[test]
    fn test_setup_script_with_git_workspace_repos() {
        let vcs = GitVcs;
        let repos = vec![LinkedRepo {
            name: "core".to_string(),
            primary_repo_path: "/home/user/Code/core".to_string(),
        }];
        let script = setup_script(&SetupScriptOptions {
            shade_setup: None,
            workspace_repos: &repos,
            workspace_label: "feat",
            vcs: &vcs,
            user: None,
            mux: None,
            paths: &[],
            detach: false,
        });

        assert!(script.contains("command -v git"));
        assert!(script.contains("git worktree add -b feat /workspace/core"));
        assert!(script.ends_with("exec /bin/bash"));
    }

    #[test]
    fn test_setup_script_without_workspace_repos() {
        let vcs = JjVcs;
        let script = setup_script(&SetupScriptOptions {
            shade_setup: None,
            workspace_repos: &[],
            workspace_label: "feat",
            vcs: &vcs,
            user: None,
            mux: None,
            paths: &[],
            detach: false,
        });

        assert!(!script.contains("command -v jj"));
        assert_eq!(script, "exec /bin/bash");
    }

    #[test]
    fn test_setup_script_shade_setup_with_user() {
        let vcs = JjVcs;
        let script = setup_script(&SetupScriptOptions {
            shade_setup: Some("npm install"),
            workspace_repos: &[],
            workspace_label: "feat",
            vcs: &vcs,
            user: Some("dev"),
            mux: None,
            paths: &[],
            detach: false,
        });

        assert!(script.contains("runuser -l dev -c 'npm install'"));
    }

    #[test]
    fn test_setup_script_shade_setup_without_user() {
        let vcs = JjVcs;
        let script = setup_script(&SetupScriptOptions {
            shade_setup: Some("npm install"),
            workspace_repos: &[],
            workspace_label: "feat",
            vcs: &vcs,
            user: None,
            mux: None,
            paths: &[],
            detach: false,
        });

        assert!(script.contains("npm install"));
        assert!(!script.contains("runuser"));
    }

    #[test]
    fn test_setup_script_drops_to_user_for_shell() {
        let vcs = JjVcs;
        let script = setup_script(&SetupScriptOptions {
            shade_setup: None,
            workspace_repos: &[],
            workspace_label: "feat",
            vcs: &vcs,
            user: Some("dev"),
            mux: None,
            paths: &[],
            detach: false,
        });

        assert!(script.ends_with("exec runuser -l dev -c 'cd /workspace && exec bash'"));
    }

    #[test]
    fn test_setup_script_drops_to_user_for_detach() {
        let vcs = JjVcs;
        let script = setup_script(&SetupScriptOptions {
            shade_setup: None,
            workspace_repos: &[],
            workspace_label: "feat",
            vcs: &vcs,
            user: Some("dev"),
            mux: None,
            paths: &[],
            detach: true,
        });

        assert!(script.contains("exec runuser -l dev -c 'sleep infinity'"));
    }

    #[test]
    fn test_prebuilt_image_name_with_vcs() {
        let name_jj = prebuilt_image_name("ubuntu:latest", None, None, "jj", None);
        let name_git = prebuilt_image_name("ubuntu:latest", None, None, "git", None);
        assert_ne!(name_jj, name_git);
    }

    #[test]
    fn test_prebuilt_image_name_with_user() {
        let name_without = prebuilt_image_name("ubuntu:latest", None, None, "jj", None);
        let name_with = prebuilt_image_name("ubuntu:latest", None, None, "jj", Some("dev"));
        assert_ne!(name_without, name_with);
    }

    #[test]
    fn test_expand_tilde_container_with_user() {
        assert_eq!(
            expand_tilde_container("~/.config", Some("dev")),
            "/home/dev/.config"
        );
    }

    #[test]
    fn test_expand_tilde_container_root() {
        assert_eq!(expand_tilde_container("~/.config", None), "/root/.config");
        assert_eq!(
            expand_tilde_container("~/.config", Some("root")),
            "/root/.config"
        );
    }

    #[test]
    fn test_expand_tilde_container_no_tilde() {
        assert_eq!(expand_tilde_container("/opt/bin", Some("dev")), "/opt/bin");
    }
}
