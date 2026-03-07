use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

use crate::container::ContainerLimits;
use crate::env_vars::{self, EnvValue};
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
    default_image: &str,
    root_env: &HashMap<String, EnvValue>,
    keychain_prefix: &str,
    container_limits: &ContainerLimits,
) -> Result<()> {
    let name = container_name(shade_name);
    let shade_config = ShadeConfig::load(shade_path)?;
    let image = shade_config.image_or(default_image);
    let merged_env = env_vars::merge_env(root_env, &shade_config.env);
    let limits = container_limits.merge(&shade_config.container);

    match inspect_container(&name)? {
        ContainerState::Running => {
            println!("Attaching to running container {name}...");
            exec_into(&name)?;
        }
        ContainerState::Stopped => {
            println!("Starting stopped container {name}...");
            start_container(&name)?;
        }
        ContainerState::NotFound => {
            let repos = find_repo_dirs(shade_path);
            let resolved = env_vars::resolve_env(&merged_env, keychain_prefix)?;

            // Use prebuilt image if available, skipping setup
            let (effective_image, effective_setup) =
                if prebuilt_image_exists(&image, shade_config.setup.as_deref()) {
                    let prebuilt = prebuilt_image_name(&image, shade_config.setup.as_deref());
                    println!("Creating container {name} from prebuilt image...");
                    (prebuilt, None)
                } else {
                    println!("Creating container {name} from {image}...");
                    (image.clone(), shade_config.setup.as_deref())
                };

            create_and_run(&CreateOptions {
                name: &name,
                shade_path,
                repos: &repos,
                image: &effective_image,
                env: &resolved,
                mounts: &shade_config.mounts,
                setup: effective_setup,
                limits: &limits,
            })?;
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

fn setup_script(setup: Option<&str>) -> String {
    match setup {
        Some(cmd) => {
            let hash = hash_setup(cmd);
            format!(
                "if [ ! -f {SETUP_MARKER} ] || [ \"$(cat {SETUP_MARKER})\" != \"{hash}\" ]; then {cmd} && echo '{hash}' > {SETUP_MARKER}; fi && exec /bin/bash"
            )
        }
        None => "exec /bin/bash".to_string(),
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
    limits: &'a ContainerLimits,
}

fn create_and_run(opts: &CreateOptions) -> Result<()> {
    let mut cmd = Command::new("docker");
    cmd.args(["run", "-it", "--name", opts.name, "-w", "/workspace"]);
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
    cmd.args(["/bin/bash", "-c", &setup_script(opts.setup)]);

    let status = cmd.status().context("failed to run docker")?;
    if !status.success() {
        bail!("docker run exited with {status}");
    }
    Ok(())
}

fn start_container(name: &str) -> Result<()> {
    let status = Command::new("docker")
        .args(["start", "-ai", name])
        .status()
        .context("failed to start container")?;
    if !status.success() {
        bail!("docker start exited with {status}");
    }
    Ok(())
}

fn exec_into(name: &str) -> Result<()> {
    let status = Command::new("docker")
        .args(["exec", "-it", name, "/bin/bash"])
        .status()
        .context("failed to exec into container")?;
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
    env: &[(String, String)],
    limits: &ContainerLimits,
) -> Result<String> {
    let image_tag = prebuilt_image_name(base_image, setup);

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

    match setup {
        Some(setup_cmd) => {
            cmd.args(["/bin/bash", "-c", setup_cmd]);
        }
        None => {
            cmd.args(["/bin/bash", "-c", "echo 'No setup command'"]);
        }
    }

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

/// Check if a prebuilt image exists for the given base image + setup combo.
pub fn prebuilt_image_exists(base_image: &str, setup: Option<&str>) -> bool {
    let tag = prebuilt_image_name(base_image, setup);
    Command::new("docker")
        .args(["image", "inspect", &tag])
        .output()
        .is_ok_and(|o| o.status.success())
}

fn prebuilt_image_name(base_image: &str, setup: Option<&str>) -> String {
    let hash = match setup {
        Some(cmd) => {
            let combined = format!("{base_image}:{cmd}");
            hash_setup(&combined)
        }
        None => hash_setup(base_image),
    };
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
