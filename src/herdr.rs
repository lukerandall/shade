//! Best-effort integration with [herdr](https://herdr.dev), a terminal agent
//! multiplexer. Shade uses herdr as an optional display layer: a shade can be
//! opened as a herdr workspace, and status badges can be pushed onto that
//! workspace so herdr's own workspace list reflects task progress.
//!
//! Every function here is best-effort. If the `herdr` binary is not installed
//! or its server is not running, callers get `Ok(None)` / `Ok(false)` rather
//! than an error — herdr is never allowed to break a shade operation.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// The `--source` id shade reports metadata under, so herdr can attribute and
/// expire shade's tokens independently of other reporters.
pub const SOURCE: &str = "shade";

/// Whether herdr is installed and its server is running. Returns `false` (never
/// errors) if the binary is missing or the server is down — callers treat that
/// as "no herdr" and carry on.
pub fn available() -> bool {
    match Command::new("herdr").arg("status").output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            server_is_running(&stdout)
        }
        Err(_) => false,
    }
}

/// Open `cwd` as a herdr workspace labelled `label`, returning the new
/// workspace id. Returns `Ok(None)` if herdr produced no parseable id.
pub fn open_workspace(cwd: &Path, label: &str) -> Result<Option<String>> {
    let output = Command::new("herdr")
        .args([
            "workspace",
            "create",
            "--no-focus",
            "--label",
            label,
            "--cwd",
        ])
        .arg(cwd)
        .output()
        .context("failed to run `herdr workspace create`")?;

    if !output.status.success() {
        anyhow::bail!(
            "herdr workspace create failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_created_workspace_id(&stdout))
}

/// Find the id of the herdr workspace whose label matches `label` (the shade
/// name). Returns `Ok(None)` if no workspace matches.
pub fn find_workspace_id(label: &str) -> Result<Option<String>> {
    let output = Command::new("herdr")
        .args(["workspace", "list"])
        .output()
        .context("failed to run `herdr workspace list`")?;

    if !output.status.success() {
        anyhow::bail!(
            "herdr workspace list failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(find_workspace_id_by_label(&stdout, label))
}

/// Push display-only metadata tokens onto a herdr workspace under shade's
/// source id. Existing shade tokens on the workspace are replaced.
pub fn report_metadata(workspace_id: &str, tokens: &[(String, String)]) -> Result<()> {
    let mut cmd = Command::new("herdr");
    cmd.args([
        "workspace",
        "report-metadata",
        workspace_id,
        "--source",
        SOURCE,
    ]);
    for (key, value) in tokens {
        cmd.arg("--token").arg(format!("{key}={value}"));
    }

    let output = cmd
        .output()
        .context("failed to run `herdr workspace report-metadata`")?;

    if !output.status.success() {
        anyhow::bail!(
            "herdr report-metadata failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

/// Close the herdr workspace whose label matches `label`, best-effort. Returns
/// `Ok(true)` if a workspace was closed, `Ok(false)` if herdr isn't running or
/// no workspace matched. Used to tear down a shade's workspace on delete.
pub fn close_workspace_by_label(label: &str) -> Result<bool> {
    if !available() {
        return Ok(false);
    }
    let Some(id) = find_workspace_id(label)? else {
        return Ok(false);
    };

    let output = Command::new("herdr")
        .args(["workspace", "close", &id])
        .output()
        .context("failed to run `herdr workspace close`")?;
    if !output.status.success() {
        anyhow::bail!(
            "herdr workspace close failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(true)
}

// --- pure parsing helpers (unit-tested) ---

/// True if `herdr status` output reports a running server.
fn server_is_running(status_output: &str) -> bool {
    // The output has a `server:` block containing `status: running`.
    let mut in_server = false;
    for line in status_output.lines() {
        let trimmed = line.trim_end();
        if trimmed.starts_with("server:") {
            in_server = true;
            continue;
        }
        // A new top-level (non-indented, non-empty) key ends the server block.
        if in_server && !trimmed.is_empty() && !trimmed.starts_with(char::is_whitespace) {
            in_server = false;
        }
        if in_server && line.trim() == "status: running" {
            return true;
        }
    }
    false
}

/// Extract `result.workspace.workspace_id` from `herdr workspace create` JSON.
fn parse_created_workspace_id(json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    value
        .get("result")?
        .get("workspace")?
        .get("workspace_id")?
        .as_str()
        .map(str::to_string)
}

/// Find `result.workspaces[].workspace_id` whose `label` equals `label` in
/// `herdr workspace list` JSON.
fn find_workspace_id_by_label(json: &str, label: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let workspaces = value.get("result")?.get("workspaces")?.as_array()?;
    workspaces
        .iter()
        .find(|w| w.get("label").and_then(|l| l.as_str()) == Some(label))
        .and_then(|w| w.get("workspace_id").and_then(|id| id.as_str()))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_running_server() {
        let out = "client:\n  version: 0.7.5\n\nserver:\n  status: running\n  version: 0.7.5\n";
        assert!(server_is_running(out));
    }

    #[test]
    fn detects_stopped_server() {
        let out = "client:\n  version: 0.7.5\n\nserver:\n  status: not running\n";
        assert!(!server_is_running(out));
    }

    #[test]
    fn ignores_running_word_outside_server_block() {
        // A `status: running` under a non-server block must not count.
        let out = "client:\n  status: running\n\nserver:\n  status: stopped\n";
        assert!(!server_is_running(out));
    }

    #[test]
    fn parses_created_workspace_id() {
        let json = r#"{"id":"cli:workspace:create","result":{"type":"workspace_created","workspace":{"label":"my-shade","number":2,"workspace_id":"w2"}}}"#;
        assert_eq!(parse_created_workspace_id(json).as_deref(), Some("w2"));
    }

    #[test]
    fn missing_workspace_id_is_none() {
        assert_eq!(parse_created_workspace_id(r#"{"result":{}}"#), None);
        assert_eq!(parse_created_workspace_id("not json"), None);
    }

    #[test]
    fn finds_workspace_id_by_label() {
        let json = r#"{"result":{"type":"workspace_list","workspaces":[
            {"label":"raven","workspace_id":"w1"},
            {"label":"my-shade","workspace_id":"w3"}
        ]}}"#;
        assert_eq!(
            find_workspace_id_by_label(json, "my-shade").as_deref(),
            Some("w3")
        );
        assert_eq!(find_workspace_id_by_label(json, "absent"), None);
    }
}
