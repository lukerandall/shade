use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

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

pub fn run_docker(shade_name: &str, shade_path: &Path, default_image: &str) -> Result<()> {
    let name = container_name(shade_name);
    let shade_config = ShadeConfig::load(shade_path)?;
    let image = shade_config.image_or(default_image);

    match inspect_container(&name)? {
        ContainerState::Running => {
            eprintln!("Attaching to running container {name}...");
            exec_into(&name)?;
        }
        ContainerState::Stopped => {
            eprintln!("Starting stopped container {name}...");
            start_container(&name)?;
        }
        ContainerState::NotFound => {
            let repos = find_repo_dirs(shade_path);
            eprintln!("Creating container {name} from {image}...");
            create_and_run(&name, shade_path, &repos, &image)?;
        }
    }

    Ok(())
}

fn create_and_run(name: &str, shade_path: &Path, repos: &[String], image: &str) -> Result<()> {
    let mut cmd = Command::new("docker");
    cmd.args(["run", "-it", "--name", name, "-w", "/workspace"]);
    cmd.args(volume_args(shade_path, repos));
    cmd.args([image, "/bin/bash"]);

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
