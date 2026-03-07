use std::collections::HashMap;

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

/// Discover repo directories inside a shade (directories containing .jj).
fn find_repo_dirs(shade_path: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(shade_path) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| e.path().join(".jj").is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect()
}

/// Build volume mount arguments for docker run.
fn volume_args(shade_path: &Path, repo_names: &[String]) -> Vec<String> {
    let mut args = Vec::new();
    for name in repo_names {
        let host_path = shade_path.join(name);
        args.push("-v".to_string());
        args.push(format!("{}:/workspace/{}", host_path.display(), name));
    }
    args
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

    match inspect_container(&name)? {
        ContainerState::Running => {
            println!("Attaching to running container {name}...");
            exec_into(&name, shade_name, mux)?;
        }
        ContainerState::Stopped => {
            println!("Starting stopped container {name}...");
            start_container(&name, mux)?;
            if mux.is_some() {
                exec_into(&name, shade_name, mux)?;
            }
        }
        ContainerState::NotFound => {
            let repos = find_repo_dirs(shade_path);
            let resolved = env_vars::resolve_env(&merged_env, keychain_prefix)?;

            // Use prebuilt image if available (setup already baked in).
            // Always pass the setup script — it checks a hash marker and
            // only re-runs if the setup command has changed.
            let effective_image =
                if prebuilt_image_exists(&docker.image, docker.setup.as_deref(), mux) {
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
                limits: &docker.limits,
                detach: mux.is_some(),
            })?;

            if mux.is_some() {
                exec_into(&name, shade_name, mux)?;
            }
        }
    }

    Ok(())
}

const SETUP_MARKER: &str = "/root/.shade-setup-hash";

fn hash_setup(cmd: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    cmd.hash(&mut hasher);
    hasher.finish()
}

fn setup_script(setup: Option<&str>, detach: bool) -> String {
    let tail = if detach {
        "exec sleep infinity"
    } else {
        "exec /bin/bash"
    };
    match setup {
        Some(cmd) => {
            let cmd = cmd.trim();
            let hash = hash_setup(cmd);
            format!(
                "if [ ! -f {SETUP_MARKER} ] || [ \"$(cat {SETUP_MARKER})\" != \"{hash}\" ]; then {cmd} && echo '{hash}' > {SETUP_MARKER}; fi && {tail}"
            )
        }
        None => tail.to_string(),
    }
}

struct CreateOptions<'a> {
    name: &'a str,
    shade_path: &'a Path,
    repos: &'a [String],
    image: &'a str,
    env: &'a [(String, String)],
    mounts: &'a [String],
    setup: Option<&'a str>,
    user: Option<&'a str>,
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
    cmd.args(volume_args(opts.shade_path, opts.repos));
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
    cmd.args(["/bin/bash", "-c", &setup_script(opts.setup, opts.detach)]);

    let status = cmd.status().context("failed to run docker")?;
    if !status.success() {
        bail!("docker run exited with {status}");
    }
    Ok(())
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

fn exec_into(name: &str, session: &str, mux: Option<&MultiplexerKind>) -> Result<()> {
    let mut cmd = Command::new("docker");
    cmd.args(["exec", "-it", name]);
    match mux {
        Some(kind) => {
            let m = kind.get();
            let attach = m.attach_cmd(session);
            let script = format!("source ~/.bashrc 2>/dev/null; {attach}");
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
