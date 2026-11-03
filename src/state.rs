//! Paths, user config, the current device and the per-device event timeline.

use crate::R;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

pub const PLUGIN_ID: &str = "herdr-ios-simulator";

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").expect("HOME is not set"))
}

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// Herdr sets HERDR_PLUGIN_STATE_DIR for plugin commands; agents calling the CLI
/// from their shell don't have it, so fall back to the same path herdr uses.
pub fn state_dir() -> PathBuf {
    env_dir("HERDR_PLUGIN_STATE_DIR")
        .unwrap_or_else(|| env_dir("XDG_STATE_HOME").unwrap_or_else(|| home().join(".local/state")).join("herdr/plugins").join(PLUGIN_ID))
}

pub fn config_dir() -> PathBuf {
    env_dir("HERDR_PLUGIN_CONFIG_DIR")
        .unwrap_or_else(|| env_dir("XDG_CONFIG_HOME").unwrap_or_else(|| home().join(".config")).join("herdr/plugins/config").join(PLUGIN_ID))
}

pub fn device_dir(udid: &str) -> PathBuf {
    state_dir().join("devices").join(udid)
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Device name or UDID that `boot` and the pane use when none is booted.
    pub default_device: Option<String>,
    /// Live view: terminal cell height / width, if the detected one is off.
    pub cell_aspect: Option<f64>,
    /// Live view: "kitty" or "blocks"; detected when unset.
    pub view_renderer: Option<String>,
    /// Live view frames per second (1-30).
    pub fps: Option<u32>,
}

impl Config {
    pub fn load() -> R<Config> {
        let path = config_dir().join("config.toml");
        match fs::read_to_string(&path) {
            Ok(s) => toml::from_str(&s).map_err(|e| format!("{}: {e}", path.display()).into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(format!("{}: {e}", path.display()).into()),
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct State {
    /// UDID of the device the CLI and pane act on.
    pub device: Option<String>,
    /// Bundle id of the app last launched through the plugin; `logs` follows it.
    pub app: Option<String>,
}

// ponytail: no lock; last writer wins, add a file lock if concurrent writers show up.
pub fn load() -> State {
    fs::read_to_string(state_dir().join("state.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn update(f: impl FnOnce(&mut State)) -> R<()> {
    let mut s = load();
    f(&mut s);
    fs::create_dir_all(state_dir())?;
    fs::write(state_dir().join("state.json"), serde_json::to_string_pretty(&s)?)?;
    Ok(())
}

pub fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// Append one event to the device's timeline.
pub fn event(udid: &str, kind: &str, detail: &str) -> R<()> {
    let dir = device_dir(udid);
    fs::create_dir_all(&dir)?;
    let mut f = fs::OpenOptions::new().create(true).append(true).open(dir.join("events.ndjson"))?;
    writeln!(f, "{}", json!({ "t": now(), "kind": kind, "detail": detail }))?;
    Ok(())
}

pub fn events(udid: &str) -> Vec<Value> {
    fs::read_to_string(device_dir(udid).join("events.ndjson")).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

pub fn clear_events(udid: &str) -> R<()> {
    match fs::remove_file(device_dir(udid).join("events.ndjson")) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

/// `--since` as a unix time: a duration (`90s`, `5m`, `2h`) or the name of a `mark`.
pub fn since(spec: &str, events: &[Value], now: f64) -> R<f64> {
    let (num, unit) = spec.split_at(spec.find(|c: char| !c.is_ascii_digit()).unwrap_or(spec.len()));
    if let Ok(n) = num.parse::<f64>() {
        let secs = match unit {
            "" | "s" => 1.0,
            "m" => 60.0,
            "h" => 3600.0,
            _ => return Err(format!("bad --since {spec:?}: use 90s, 5m, 2h or a mark name").into()),
        };
        return Ok(now - n * secs);
    }
    events
        .iter()
        .rev()
        .find(|e| e["kind"] == "mark" && e["detail"] == spec)
        .and_then(|e| e["t"].as_f64())
        .ok_or_else(|| format!("no mark named {spec:?}; set one with `herdr-ios mark {spec:?}`").into())
}

/// Local wall-clock `HH:MM:SS` for a unix time.
pub fn clock(t: f64) -> String {
    let secs = t as libc::time_t;
    // SAFETY: localtime_r writes only into `tm`, which outlives the call.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&secs, &mut tm) };
    format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn since_reads_durations() {
        assert_eq!(since("5m", &[], 1000.0).unwrap(), 700.0);
        assert_eq!(since("30", &[], 1000.0).unwrap(), 970.0);
    }

    #[test]
    fn since_finds_the_latest_mark() {
        let ev = [json!({"t": 10.0, "kind": "mark", "detail": "login"}), json!({"t": 20.0, "kind": "mark", "detail": "login"})];
        assert_eq!(since("login", &ev, 99.0).unwrap(), 20.0);
    }

    #[test]
    fn since_rejects_unknown_marks() {
        assert!(since("nope", &[], 0.0).is_err());
    }
}
