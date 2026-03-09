use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

use crate::container::{ContainerLimits, DockerConfig};
use crate::env_vars::{self, EnvValue};
use crate::multiplexer::MultiplexerKind;
use crate::shade_config::ShadeConfig;

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

/// Info about a repo inside a shade, used for building Docker mounts.
struct RepoMount {
    /// Repo name (e.g. "core" or "acme/core").
    name: String,
    /// Whether this is a jj workspace (true) or an independent clone (false).
    is_workspace: bool,
}

/// Resolve the primary repo root for a jj workspace.
///
/// A workspace's `.jj/repo` is a file containing a relative path to the
/// primary's `.jj/repo` directory. We canonicalize that and go up two levels
/// to get the repo root.
fn resolve_primary_repo(workspace_path: &Path) -> Option<std::path::PathBuf> {
    let repo_file = workspace_path.join(".jj/repo");
    if !repo_file.is_file() {
        return None;
    }
    let rel = std::fs::read_to_string(&repo_file).ok()?;
    let jj_dir = workspace_path.join(".jj");
    let primary_repo = jj_dir.join(rel.trim()).canonicalize().ok()?;
    // primary_repo is the .jj/repo dir; go up twice to get the repo root
    primary_repo.parent()?.parent().map(|p| p.to_path_buf())
}

/// Build volume mount arguments and an optional workspace symlink command for docker run.
///
/// Clone repos are mounted at `/workspace/<name>` as before.
/// Workspace repos are mounted at their host absolute paths so that the relative
/// path in `.jj/repo` resolves correctly inside the container. A symlink from
/// `/workspace` to the shade's host path connects everything up.
fn volume_args(shade_path: &Path, repos: &[RepoMount]) -> (Vec<String>, Option<String>) {
    let mut args = Vec::new();
    let has_workspace = repos.iter().any(|r| r.is_workspace);

    if has_workspace {
        // Mount the shade dir at its host absolute path
        let shade_abs = shade_path.to_string_lossy();
        args.push("-v".to_string());
        args.push(format!("{shade_abs}:{shade_abs}"));

        // Mount each workspace's primary repo at its host absolute path
        let mut mounted_primaries = HashSet::new();
        for repo in repos.iter().filter(|r| r.is_workspace) {
            let ws_path = shade_path.join(&repo.name);
            if let Some(primary) = resolve_primary_repo(&ws_path) {
                let primary_str = primary.to_string_lossy().to_string();
                if mounted_primaries.insert(primary_str.clone()) {
                    args.push("-v".to_string());
                    args.push(format!("{primary_str}:{primary_str}"));
                }
            }
        }
    }

    // Clone repos get the simple /workspace/<name> mount
    for repo in repos.iter().filter(|r| !r.is_workspace) {
        let host_path = shade_path.join(&repo.name);
        args.push("-v".to_string());
        args.push(format!("{}:/workspace/{}", host_path.display(), repo.name));
    }

    // If we have workspace repos, symlink /workspace -> shade host path so
    // the working directory resolves correctly
    let symlink_cmd = if has_workspace {
        Some(format!(
            "ln -sfn {} /workspace",
            shade_path.to_string_lossy()
        ))
    } else {
        None
    };

    (args, symlink_cmd)
}

pub fn run_docker(
    shade_name: &str,
    shade_path: &Path,
    root_docker: &DockerConfig,
    root_env: &HashMap<String, EnvValue>,
    keychain_prefix: &str,
) -> Result<()> {
    let name = container_name(shade_name);
    let shade_config = ShadeConfig::load(shade_path)?;
    let docker = root_docker.merge(&shade_config.docker);
    let merged_env = env_vars::merge_env(root_env, &shade_config.env);

    let mux = docker.multiplexer.as_ref();

    let paths = &docker.path;

    match inspect_container(&name)? {
        ContainerState::Running => {
            println!("Attaching to running container {name}...");
            exec_into(&name, shade_name, mux, paths)?;
        }
        ContainerState::Stopped => {
            println!("Starting stopped container {name}...");
            start_container(&name, mux)?;
            if mux.is_some() {
                wait_for_ready(&name)?;
                exec_into(&name, shade_name, mux, paths)?;
            }
        }
        ContainerState::NotFound => {
            let repo_names = crate::env::list_workspace_dirs(shade_path);
            let repos: Vec<RepoMount> = repo_names
                .into_iter()
                .map(|name| {
                    let is_workspace = crate::env::is_jj_workspace(&shade_path.join(&name));
                    RepoMount { name, is_workspace }
                })
                .collect();
            let resolved = env_vars::resolve_env(&merged_env, keychain_prefix)?;

            let has_prebuilt = prebuilt_image_exists(&docker.image, docker.setup.as_deref(), mux);

            if !has_prebuilt && (docker.setup.is_some() || mux.is_some()) {
                bail!(
                    "no prebuilt image found. Run `shade docker build` first to bake in setup and tools"
                );
            }

            let effective_image = if has_prebuilt {
                let prebuilt = prebuilt_image_name(&docker.image, docker.setup.as_deref(), mux);
                println!("Creating container {name} from prebuilt image...");
                prebuilt
            } else {
                println!("Creating container {name} from {}...", docker.image);
                docker.image.clone()
            };

            create_and_run(&CreateOptions {
                name: &name,
                shade_path,
                repos: &repos,
                image: &effective_image,
                env: &resolved,
                mounts: &docker.mounts,
                setup: docker.setup.as_deref(),
                user: docker.user.as_deref(),
                multiplexer: mux,
                paths,
                limits: &docker.limits,
                detach: mux.is_some(),
            })?;

            if mux.is_some() {
                wait_for_ready(&name)?;
                exec_into(&name, shade_name, mux, paths)?;
            }
        }
    }

    Ok(())
}

const SETUP_MARKER: &str = "/root/.shade-setup-hash";
const READY_MARKER: &str = "/root/.shade-ready";

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
    // Quote each path individually so spaces and special characters don't
    // break the shell snippet. Use double quotes to allow variable expansion
    // (e.g., $HOME). Escape backslashes, double quotes, and backticks that
    // would break the double-quote context. Dollar signs are NOT escaped to
    // allow shell variable expansion.
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

fn setup_script(
    setup: Option<&str>,
    mux: Option<&MultiplexerKind>,
    paths: &[String],
    detach: bool,
    symlink_cmd: Option<&str>,
) -> String {
    let tail = if detach {
        "exec sleep infinity"
    } else {
        "exec /bin/bash"
    };

    let mux_install = mux.map(|kind| {
        let m = kind.get();
        let cmd = m.install_cmd();
        format!("command -v {} >/dev/null 2>&1 || {{ {cmd}; }}", m.name())
    });

    let mut parts = Vec::new();

    if let Some(sl) = symlink_cmd {
        parts.push(sl.to_string());
    }

    if let Some(export) = path_export(paths) {
        parts.push(export);
    }

    if let Some(cmd) = setup {
        let cmd = cmd.trim();
        let hash = hash_setup(cmd);
        parts.push(format!(
            "if [ ! -f {SETUP_MARKER} ] || [ \"$(cat {SETUP_MARKER})\" != \"{hash}\" ]; then {cmd} && echo '{hash}' > {SETUP_MARKER}; fi"
        ));
    }

    if let Some(install) = mux_install {
        parts.push(install);
    }

    if detach {
        parts.push(format!("touch {READY_MARKER}"));
    }

    parts.push(tail.to_string());
    parts.join(" && ")
}

struct CreateOptions<'a> {
    name: &'a str,
    shade_path: &'a Path,
    repos: &'a [RepoMount],
    image: &'a str,
    env: &'a [(String, String)],
    mounts: &'a [String],
    setup: Option<&'a str>,
    user: Option<&'a str>,
    multiplexer: Option<&'a MultiplexerKind>,
    paths: &'a [String],
    limits: &'a ContainerLimits,
    detach: bool,
}

fn create_and_run(opts: &CreateOptions) -> Result<()> {
    let mut cmd = Command::new("docker");
    if opts.detach {
        cmd.args(["run", "-d", "--name", opts.name, "-w", "/workspace"]);
    } else {
        cmd.args(["run", "-it", "--name", opts.name, "-w", "/workspace"]);
    }
    if let Some(user) = opts.user {
        cmd.args(["-u", user]);
    }
    cmd.args(opts.limits.docker_args());
    let (vol_args, symlink_cmd) = volume_args(opts.shade_path, opts.repos);
    cmd.args(vol_args);
    for mount in opts.mounts {
        let mount_arg = if mount.contains(':') {
            mount.to_string()
        } else {
            format!("{mount}:{mount}")
        };
        cmd.args(["-v", &mount_arg]);
    }
    for (key, value) in opts.env {
        cmd.args(["-e", &format!("{key}={value}")]);
    }
    cmd.arg(opts.image);
    let script = setup_script(
        opts.setup,
        opts.multiplexer,
        opts.paths,
        opts.detach,
        symlink_cmd.as_deref(),
    );
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
        // Detached container — just start it in background
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
) -> Result<()> {
    let mut cmd = Command::new("docker");
    cmd.args(["exec", "-it", name]);
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

/// Build a pre-configured image by running setup on the base image and committing.
/// Returns the image name.
pub fn build_image(
    base_image: &str,
    setup: Option<&str>,
    multiplexer: Option<&MultiplexerKind>,
    env: &[(String, String)],
    limits: &ContainerLimits,
) -> Result<String> {
    let image_tag = prebuilt_image_name(base_image, setup, multiplexer);

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

    // Run the setup in a temporary container (no security limits — we need
    // full capabilities for apt-get, etc. Hardening applies to runtime only.)
    let mut cmd = Command::new("docker");
    cmd.args(["run", "--name", &temp_name]);
    let _ = limits; // limits are for runtime containers, not build
    for (key, value) in env {
        cmd.args(["-e", &format!("{key}={value}")]);
    }
    cmd.arg(base_image);

    // Build a script that runs setup + installs the multiplexer
    let mut steps = Vec::new();
    if let Some(setup_cmd) = setup {
        let setup_cmd = setup_cmd.trim();
        let hash = hash_setup(setup_cmd);
        steps.push(format!("{setup_cmd} && echo '{hash}' > {SETUP_MARKER}"));
    }
    if let Some(mux_kind) = multiplexer {
        let mux = mux_kind.get();
        println!("Including {} in image...", mux.name());
        steps.push(mux.install_cmd().to_string());
    }
    if steps.is_empty() {
        steps.push("echo 'No setup command'".to_string());
    }
    let build_script = steps.join(" && ");
    cmd.args(["/bin/bash", "-c", &build_script]);

    println!("Running setup in temporary container...");
    let status = cmd.status().context("failed to run docker")?;
    if !status.success() {
        // Clean up the failed container
        let _ = Command::new("docker")
            .args(["rm", "-f", &temp_name])
            .output();
        bail!("setup command failed with {status}");
    }

    // Commit the container as a new image
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

    // Remove the temporary container
    let _ = Command::new("docker").args(["rm", &temp_name]).output();

    println!("Built image {image_tag}");
    Ok(image_tag)
}

/// Check if a prebuilt image exists for the given base image + setup + multiplexer combo.
pub fn prebuilt_image_exists(
    base_image: &str,
    setup: Option<&str>,
    multiplexer: Option<&MultiplexerKind>,
) -> bool {
    let tag = prebuilt_image_name(base_image, setup, multiplexer);
    Command::new("docker")
        .args(["image", "inspect", &tag])
        .output()
        .is_ok_and(|o| o.status.success())
}

fn prebuilt_image_name(
    base_image: &str,
    setup: Option<&str>,
    multiplexer: Option<&MultiplexerKind>,
) -> String {
    let mux_str = match multiplexer {
        Some(kind) => kind.get().name().to_string(),
        None => String::new(),
    };
    let combined = format!(
        "{base_image}:{setup}:{mux_str}",
        setup = setup.unwrap_or("")
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

    #[test]
    fn test_path_export_allows_variable_expansion() {
        let paths = vec![
            "$HOME/.cargo/bin".to_string(),
            "$HOME/.local/bin".to_string(),
        ];
        let export = path_export(&paths).unwrap();
        // Variables should not be escaped
        assert!(export.contains("$HOME"));
        // Should use double quotes to allow expansion
        assert!(export.contains(r#""$HOME/.cargo/bin""#));
    }

    #[test]
    fn test_path_export_escapes_special_chars() {
        let paths = vec![
            r#"/path/with"quotes"#.to_string(),
            r"/path/with\backslash".to_string(),
        ];
        let export = path_export(&paths).unwrap();
        // Double quotes should be escaped
        assert!(export.contains(r#"with\"quotes"#));
        // Backslashes should be escaped
        assert!(export.contains(r"with\\backslash"));
    }

    #[test]
    fn test_path_export_handles_backticks() {
        let paths = vec!["/path/with`backtick`".to_string()];
        let export = path_export(&paths).unwrap();
        // Backticks should be escaped to prevent command substitution
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
    fn test_volume_args_clone_repos_mounted_at_workspace() {
        let shade = Path::new("/home/user/Shades/2026-03-09-test");
        let repos = vec![
            RepoMount {
                name: "core".to_string(),
                is_workspace: false,
            },
            RepoMount {
                name: "acme/dashboard".to_string(),
                is_workspace: false,
            },
        ];

        let (args, symlink) = volume_args(shade, &repos);

        // Clone repos should be mounted at /workspace/<name>
        assert!(args.contains(&format!("{}/core:/workspace/core", shade.display())));
        assert!(args.contains(&format!(
            "{}/acme/dashboard:/workspace/acme/dashboard",
            shade.display()
        )));
        // No symlink needed for clone-only
        assert!(symlink.is_none());
    }

    #[test]
    fn test_volume_args_workspace_repos_mounted_at_host_path() {
        let shade = Path::new("/home/user/Shades/2026-03-09-test");
        let repos = vec![RepoMount {
            name: "core".to_string(),
            is_workspace: true,
        }];

        let (args, symlink) = volume_args(shade, &repos);

        // Shade dir should be mounted at its host path
        assert!(args.contains(&format!("{}:{}", shade.display(), shade.display())));
        // Symlink should point /workspace to the shade host path
        assert!(symlink.is_some());
        assert!(
            symlink
                .unwrap()
                .contains(&shade.to_string_lossy().to_string())
        );
    }

    #[test]
    fn test_volume_args_mixed_repos() {
        let shade = Path::new("/home/user/Shades/2026-03-09-test");
        let repos = vec![
            RepoMount {
                name: "ws-repo".to_string(),
                is_workspace: true,
            },
            RepoMount {
                name: "clone-repo".to_string(),
                is_workspace: false,
            },
        ];

        let (args, symlink) = volume_args(shade, &repos);

        // Shade dir mounted for workspace repos
        assert!(args.contains(&format!("{}:{}", shade.display(), shade.display())));
        // Clone repo gets /workspace/ mount
        assert!(args.contains(&format!(
            "{}/clone-repo:/workspace/clone-repo",
            shade.display()
        )));
        // Symlink is needed because of workspace repo
        assert!(symlink.is_some());
    }
}
