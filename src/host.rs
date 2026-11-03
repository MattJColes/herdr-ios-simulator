//! The two herdr front ends the View and agents can live in, behind one interface:
//! - `Tui`: the `herdr` terminal server (plugin panes, `herdr agent prompt`).
//! - `Desktop`: Herdr.app on herdr-core (`herdr-app` panes, `hc send`).
//!
//! Copied from herdr-web-browser and trimmed to what this plugin uses.

use crate::R;
use crate::state;
use serde_json::Value;
use std::io::Read;
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

const CLI_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Host {
    Tui,
    Desktop,
}

fn env(var: &str) -> Option<String> {
    std::env::var(var).ok().filter(|v| !v.is_empty())
}

fn herdr_bin() -> String {
    env("HERDR_BIN_PATH").unwrap_or_else(|| "herdr".into())
}

/// The caller's host from the env each host injects, else the desktop when its window
/// answers, else the TUI.
pub fn host() -> Host {
    if env("HERDR_PANE_ID").is_some() {
        Host::Tui
    } else if env("HC_TERMINAL_ID").is_some() || run("herdr-app", &["status"]).is_ok() {
        Host::Desktop
    } else {
        Host::Tui
    }
}

/// Run a CLI with a deadline; parse stdout as JSON (Null when not JSON).
pub fn run(bin: &str, args: &[&str]) -> R<Value> {
    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot run {bin}: {e}"))?;
    let mut out = child.stdout.take().expect("piped");
    let mut err = child.stderr.take().expect("piped");
    let out_t = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = out.read_to_string(&mut s);
        s
    });
    let err_t = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = err.read_to_string(&mut s);
        s
    });
    let deadline = Instant::now() + CLI_TIMEOUT;
    let status = loop {
        if let Some(st) = child.try_wait()? {
            break st;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            return Err(format!("{bin} {} timed out", args.join(" ")).into());
        }
        sleep(Duration::from_millis(20));
    };
    let (stdout, stderr) = (out_t.join().unwrap_or_default(), err_t.join().unwrap_or_default());
    if !status.success() {
        let msg = if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() };
        return Err(format!("{bin} {}: {msg}", args.first().unwrap_or(&"")).into());
    }
    Ok(serde_json::from_str(&stdout).unwrap_or(Value::Null))
}

fn herdr(args: &[&str]) -> R<Value> {
    run(&herdr_bin(), args)
}

fn s(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or_default().to_string()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Agent {
    /// Name if the agent has one, else its pane id or terminal address; all are valid targets.
    pub id: String,
    pub workspace_id: String,
}

/// `herdr agent list`.
pub fn parse_tui_agents(v: &Value) -> Vec<Agent> {
    v.pointer("/result/agents")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|a| Agent {
            id: a["name"].as_str().filter(|n| !n.is_empty()).map(String::from).unwrap_or_else(|| s(a, "pane_id")),
            workspace_id: s(a, "workspace_id"),
        })
        .collect()
}

/// `herdr-app ls`: terminals that report a status are agents; also returns the active space.
pub fn parse_desktop_agents(v: &Value) -> (Vec<Agent>, Option<String>) {
    let mut agents = Vec::new();
    let mut active = None;
    for sp in v["spaces"].as_array().into_iter().flatten() {
        if sp["active"] == true {
            active = Some(s(sp, "id"));
        }
        for tab in sp["tabs"].as_array().into_iter().flatten() {
            for t in tab["terminals"].as_array().into_iter().flatten().filter(|t| t["status"].is_string()) {
                agents.push(Agent { id: s(t, "terminal"), workspace_id: s(sp, "id") });
            }
        }
    }
    (agents, active)
}

/// Live agents on `host`, the focused workspace/space, and (desktop) the last focused agent.
pub fn agents(host: Host) -> R<(Vec<Agent>, Option<String>, Option<String>)> {
    match host {
        Host::Tui => {
            let agents = parse_tui_agents(&herdr(&["agent", "list"])?);
            let ws = herdr(&["workspace", "list"])?;
            let focused = ws
                .pointer("/result/workspaces")
                .and_then(Value::as_array)
                .and_then(|w| w.iter().find(|w| w["focused"] == true))
                .map(|w| s(w, "workspace_id"));
            Ok((agents, focused, None))
        }
        Host::Desktop => {
            let ls = run("herdr-app", &["ls"])?;
            let (agents, active) = parse_desktop_agents(&ls);
            // Called from a terminal (the View, or an agent): the agent in its own space, not
            // whichever agent was focused last, which may be in another project entirely.
            if let Some(own) = env("HC_TERMINAL_ID").and_then(|t| space_of(&ls, &t)) {
                return Ok((agents, Some(own), None));
            }
            let last = run("herdr-app", &["status"]).ok().and_then(|v| v["agent"].as_str().map(String::from));
            Ok((agents, active, last))
        }
    }
}

/// Submit `text` to the agent's prompt.
pub fn prompt(host: Host, target: &str, text: &str) -> R<()> {
    match host {
        Host::Tui => herdr(&["agent", "prompt", target, text]).map(drop),
        Host::Desktop => run("hc", &["send", target, text]).map(drop),
    }
}

/// Best effort, TUI only: herdr desktop has no CLI for toasts.
pub fn notify(host: Host, title: &str, body: &str) {
    if host == Host::Tui {
        drop(herdr(&["notification", "show", title, "--body", body]));
    }
}

/// The herdr desktop space that holds `terminal`, from `herdr-app ls`.
fn space_of(ls: &Value, terminal: &str) -> Option<String> {
    ls["spaces"]
        .as_array()?
        .iter()
        .find(|sp| {
            sp["tabs"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|t| t["terminals"].as_array().into_iter().flatten())
                .any(|t| t["terminal"] == terminal)
        })
        .map(|sp| s(sp, "id"))
}

/// The herdr desktop pane that holds `terminal`, from `herdr-app ls`.
fn desktop_pane(ls: &Value, terminal: &str) -> Option<String> {
    ls["spaces"]
        .as_array()?
        .iter()
        .flat_map(|sp| sp["tabs"].as_array().into_iter().flatten())
        .flat_map(|tab| tab["terminals"].as_array().into_iter().flatten())
        .find(|t| t["terminal"] == terminal)
        .map(|t| s(t, "pane"))
        .filter(|p| !p.is_empty())
}

/// Open the View: a plugin pane in the TUI; in herdr desktop, a split right of the calling
/// terminal (or the last focused agent).
pub fn open_view(host: Host) -> R<String> {
    match host {
        Host::Tui => {
            herdr(&["plugin", "pane", "open", "--plugin", state::PLUGIN_ID, "--entrypoint", "simulator"])?;
            Ok("showing the simulator in a herdr pane".into())
        }
        Host::Desktop => {
            let exe = std::fs::canonicalize(std::env::current_exe()?)?.display().to_string();
            let argv = serde_json::to_string(&[exe.as_str(), "pane"])?;
            let beside = match env("HC_TERMINAL_ID") {
                Some(t) => Some(t),
                None => run("herdr-app", &["status"])?["agent"].as_str().map(String::from),
            };
            let mut args = vec!["do", "pane.split", "--direction", "right", "--argv", &argv];
            let ls = run("herdr-app", &["ls"])?;
            let pane = beside.and_then(|t| desktop_pane(&ls, &t));
            if let Some(p) = &pane {
                args.extend(["--pane", p]);
            }
            let v = run("herdr-app", &args)?;
            Ok(format!("showing the simulator in herdr desktop terminal {}", v["terminal"].as_str().unwrap_or("?")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tui_agents_use_their_name_else_pane() {
        let v = json!({ "result": { "agents": [
            { "agent": "claude", "pane_id": "w1:p1", "workspace_id": "w1" },
            { "agent": "codex", "name": "reviewer", "pane_id": "w2:p1", "workspace_id": "w2" } ]}});
        let ids: Vec<_> = parse_tui_agents(&v).into_iter().map(|a| a.id).collect();
        assert_eq!(ids, ["w1:p1", "reviewer"]);
    }

    #[test]
    fn desktop_agents_are_terminals_with_status() {
        let v = json!({ "spaces": [
            { "id": "sp1", "active": false, "tabs": [{ "terminals": [
                { "terminal": "t3", "pane": "p3", "status": "blocked" }, { "terminal": "t4", "status": null } ] }] },
            { "id": "sp2", "active": true, "tabs": [{ "terminals": [{ "terminal": "t5", "pane": "p5", "status": "idle" }] }] } ] });
        let (agents, active) = parse_desktop_agents(&v);
        assert_eq!(agents.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), ["t3", "t5"]);
        assert_eq!(active.as_deref(), Some("sp2"));
        assert_eq!(desktop_pane(&v, "t5").as_deref(), Some("p5"));
        assert_eq!(space_of(&v, "t4").as_deref(), Some("sp1"));
        assert_eq!(space_of(&v, "t9"), None);
    }
}
