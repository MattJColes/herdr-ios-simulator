//! The simulator through `xcrun simctl`: devices, apps, logs and crash reports.

use crate::R;
use crate::state::{self, Config};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Run `xcrun simctl ARGS`, returning stdout; stderr becomes the error.
pub fn simctl(args: &[&str]) -> R<String> {
    let out = Command::new("xcrun").arg("simctl").args(args).stdin(Stdio::null()).output().map_err(|e| format!("cannot run xcrun: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("simctl {}: {}", args.first().unwrap_or(&""), err.trim()).into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    pub udid: String,
    pub name: String,
    /// e.g. "iOS 27.2"
    pub os: String,
    pub booted: bool,
}

/// Available devices from `simctl list devices -j`, newest OS first.
pub fn parse_devices(v: &Value) -> Vec<Device> {
    let mut out = Vec::new();
    for (runtime, list) in v["devices"].as_object().into_iter().flatten() {
        let os = os_name(runtime);
        for d in list.as_array().into_iter().flatten().filter(|d| d["isAvailable"] != false) {
            out.push(Device {
                udid: d["udid"].as_str().unwrap_or_default().into(),
                name: d["name"].as_str().unwrap_or_default().into(),
                os: os.clone(),
                booted: d["state"] == "Booted",
            });
        }
    }
    out.sort_by(|a, b| version_key(&b.os).cmp(&version_key(&a.os)).then(a.name.cmp(&b.name)));
    out
}

/// `com.apple.CoreSimulator.SimRuntime.iOS-27-2` -> `iOS 27.2`; device-specific runtimes
/// such as `iOS-27-2-iPhone19-4` drop the suffix.
fn os_name(runtime: &str) -> String {
    let id = runtime.split_once("SimRuntime.").map_or(runtime, |(_, id)| id);
    let mut parts = id.split('-');
    let platform = parts.next().unwrap_or(id);
    let version: Vec<&str> = parts.take_while(|p| p.parse::<u32>().is_ok()).collect();
    format!("{platform} {}", version.join("."))
}

fn version_key(os: &str) -> (bool, Vec<u32>) {
    let nums = os.split(|c: char| !c.is_ascii_digit()).filter_map(|n| n.parse().ok()).collect();
    (os.starts_with("iOS"), nums)
}

pub fn devices() -> R<Vec<Device>> {
    Ok(parse_devices(&serde_json::from_str(&simctl(&["list", "devices", "available", "-j"])?)?))
}

/// Find a device by UDID or name (exact, then case-insensitive prefix).
pub fn find<'a>(all: &'a [Device], q: &str) -> Option<&'a Device> {
    let lq = q.to_lowercase();
    all.iter()
        .find(|d| d.udid == q || d.name == q)
        .or_else(|| all.iter().find(|d| d.name.to_lowercase() == lq))
        .or_else(|| all.iter().find(|d| d.name.to_lowercase().starts_with(&lq)))
}

/// The device to act on: `--device`, else the current one if booted, else the only booted one.
pub fn current(explicit: Option<&str>) -> R<Device> {
    let all = devices()?;
    if let Some(q) = explicit {
        return find(&all, q).cloned().ok_or_else(|| format!("no simulator named {q:?}; see `herdr-ios-simulator devices`").into());
    }
    let cur = state::load().device;
    if let Some(d) = all.iter().find(|d| d.booted && Some(&d.udid) == cur.as_ref()) {
        return Ok(d.clone());
    }
    match all.iter().filter(|d| d.booted).collect::<Vec<_>>()[..] {
        [d] => Ok(d.clone()),
        [] => Err("no simulator is booted; run `herdr-ios-simulator boot`".into()),
        _ => Err("several simulators are booted; pass --device or run `herdr-ios-simulator boot <name>`".into()),
    }
}

/// Boot `q` (or the configured default, the last used, or the newest iPhone) and make it current.
pub fn boot(cfg: &Config, q: Option<&str>) -> R<Device> {
    let all = devices()?;
    let last = state::load().device;
    let d = match q.or(cfg.default_device.as_deref()) {
        Some(q) => find(&all, q).ok_or_else(|| format!("no simulator named {q:?}; see `herdr-ios-simulator devices`"))?,
        None => all
            .iter()
            .find(|d| Some(&d.udid) == last.as_ref())
            .or_else(|| all.iter().find(|d| d.booted))
            .or_else(|| all.iter().find(|d| d.name.starts_with("iPhone")))
            .ok_or("no iOS simulators installed; add one in Xcode > Settings > Components")?,
    }
    .clone();
    if !d.booted {
        // bootstatus -b boots and waits until the device is usable.
        simctl(&["bootstatus", &d.udid, "-b"])?;
        state::event(&d.udid, "boot", &format!("{} ({})", d.name, d.os))?;
    }
    state::update(|s| s.device = Some(d.udid.clone()))?;
    Ok(Device { booted: true, ..d })
}

pub fn shutdown(d: &Device) -> R<()> {
    simctl(&["shutdown", &d.udid])?;
    state::event(&d.udid, "shutdown", &d.name)
}

pub fn install(d: &Device, app: &Path) -> R<()> {
    simctl(&["install", &d.udid, &app.to_string_lossy()])?;
    state::event(&d.udid, "install", &app.display().to_string())
}

fn output_paths(udid: &str) -> (PathBuf, PathBuf) {
    let dir = state::device_dir(udid);
    (dir.join("stdout.log"), dir.join("stderr.log"))
}

/// Launch (relaunching if running) with stdout/stderr captured; returns the pid.
pub fn launch(d: &Device, bundle: &str) -> R<String> {
    let (out, err) = output_paths(&d.udid);
    std::fs::create_dir_all(state::device_dir(&d.udid))?;
    let (o, e) = (format!("--stdout={}", out.display()), format!("--stderr={}", err.display()));
    let r = simctl(&["launch", "--terminate-running-process", &o, &e, &d.udid, bundle])?;
    // "com.example.MyApp: 12345"
    let pid = r.trim().rsplit(':').next().unwrap_or("").trim().to_string();
    state::update(|s| s.app = Some(bundle.into()))?;
    state::event(&d.udid, "launch", &format!("{bundle} pid {pid}"))?;
    Ok(pid)
}

pub fn terminate(d: &Device, bundle: &str) -> R<()> {
    simctl(&["terminate", &d.udid, bundle])?;
    state::event(&d.udid, "terminate", bundle)
}

pub fn openurl(d: &Device, url: &str) -> R<()> {
    simctl(&["openurl", &d.udid, url])?;
    state::event(&d.udid, "openurl", url)
}

pub fn screenshot(d: &Device, path: &Path) -> R<()> {
    simctl(&["io", &d.udid, "screenshot", "--type=png", &path.to_string_lossy()]).map(drop)
}

/// The app's executable name, which is the process name in the unified log.
fn executable(d: &Device, bundle: &str) -> R<String> {
    let app = simctl(&["get_app_container", &d.udid, bundle, "app"])?;
    let plist = Path::new(app.trim()).join("Info.plist");
    let out = Command::new("plutil").args(["-extract", "CFBundleExecutable", "raw", "-o", "-"]).arg(&plist).output()?;
    if !out.status.success() {
        return Err(format!("cannot read {}", plist.display()).into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub struct LogQuery<'a> {
    pub bundle: Option<&'a str>,
    pub since: f64,
    pub errors: bool,
    pub info: bool,
}

/// Unified log entries from the device (ndjson objects), oldest first.
pub fn logs(d: &Device, q: &LogQuery) -> R<Vec<Value>> {
    let last = format!("{}s", (state::now() - q.since).ceil().max(1.0));
    let mut args = vec!["spawn", &d.udid, "log", "show", "--style", "ndjson", "--last", &last];
    if q.info {
        args.push("--info");
    }
    let mut preds = Vec::new();
    if let Some(b) = q.bundle {
        preds.push(format!("process == \"{}\"", executable(d, b)?.replace('"', "")));
    }
    if q.errors {
        preds.push("messageType >= error".into());
    }
    let pred = preds.join(" AND ");
    if !pred.is_empty() {
        args.extend(["--predicate", &pred]);
    }
    let out = simctl(&args)?;
    Ok(out.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).filter(|v| v.get("eventMessage").is_some()).collect())
}

/// One log entry as `HH:MM:SS.mmm level   [subsystem] message`.
pub fn log_line(v: &Value) -> String {
    // "2026-11-03 21:04:31.123456+1100" -> "21:04:31.123"
    let ts = v["timestamp"].as_str().unwrap_or("");
    let time = ts.split(' ').nth(1).map(|t| t.get(..12).unwrap_or(t)).unwrap_or(ts);
    let level = match v["messageType"].as_str().unwrap_or("") {
        "Fault" => "fault",
        "Error" => "error",
        "Info" => "info",
        "Debug" => "debug",
        _ => "log",
    };
    let sub =
        [v["subsystem"].as_str().unwrap_or(""), v["category"].as_str().unwrap_or("")].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>();
    let tag = if sub.is_empty() { String::new() } else { format!("[{}] ", sub.join(":")) };
    format!("{time} {level:<5} {tag}{}", v["eventMessage"].as_str().unwrap_or("").trim_end())
}

/// The last `max` lines the app wrote to stdout and stderr since it was launched.
pub fn output(udid: &str, max: usize) -> Vec<String> {
    let (out, err) = output_paths(udid);
    let mut lines = Vec::new();
    for (tag, p) in [("stdout", out), ("stderr", err)] {
        let s = std::fs::read_to_string(p).unwrap_or_default();
        let all: Vec<&str> = s.lines().collect();
        lines.extend(all[all.len().saturating_sub(max)..].iter().map(|l| format!("{tag} {l}")));
    }
    lines
}

/// Summarise an `.ips` crash report: app, time, exception and the crashing thread's top frames.
pub fn parse_crash(text: &str) -> Option<Value> {
    let (head, body) = text.split_once('\n')?;
    let (h, b): (Value, Value) = (serde_json::from_str(head).ok()?, serde_json::from_str(body).ok()?);
    let images = b["usedImages"].as_array().cloned().unwrap_or_default();
    let thread = b["faultingThread"].as_u64().unwrap_or(0) as usize;
    let frames: Vec<String> = b["threads"][thread]["frames"]
        .as_array()
        .into_iter()
        .flatten()
        .take(8)
        .map(|f| {
            let image = f["imageIndex"].as_u64().and_then(|i| images.get(i as usize)).and_then(|i| i["name"].as_str()).unwrap_or("?");
            let sym = f["symbol"].as_str().unwrap_or("???");
            match (f["sourceFile"].as_str(), f["sourceLine"].as_u64()) {
                (Some(file), Some(line)) => format!("{image}  {sym}  {file}:{line}"),
                _ => format!("{image}  {sym}"),
            }
        })
        .collect();
    let ex = &b["exception"];
    let reason = b["asi"]
        .as_object()
        .and_then(|a| a.values().flat_map(|v| v.as_array().cloned().unwrap_or_default()).next())
        .and_then(|v| v.as_str().map(String::from));
    Some(json!({
        "app": h["app_name"],
        "bundle": h["bundleID"],
        "time": h["timestamp"],
        "exception": format!("{} ({})", ex["type"].as_str().unwrap_or("?"), ex["signal"].as_str().unwrap_or("?")),
        "reason": reason,
        "frames": frames,
    }))
}

/// Crash reports written by apps on this device, newest first.
pub fn crashes(udid: &str, since: f64) -> R<Vec<Value>> {
    let dir = PathBuf::from(std::env::var_os("HOME").ok_or("HOME is not set")?).join("Library/Logs/DiagnosticReports");
    let mut found = Vec::new();
    for entry in std::fs::read_dir(&dir)?.flatten() {
        let p = entry.path();
        let modified = entry.metadata().and_then(|m| m.modified()).ok().and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok());
        let Some(m) = modified.map(|m| m.as_secs_f64()).filter(|m| *m >= since) else { continue };
        if p.extension().is_none_or(|e| e != "ips") {
            continue;
        }
        // Simulator app paths contain .../CoreSimulator/Devices/<UDID>/...
        let Ok(text) = std::fs::read_to_string(&p) else { continue };
        if !text.contains(udid) {
            continue;
        }
        if let Some(mut c) = parse_crash(&text) {
            c["file"] = json!(p.display().to_string());
            found.push((m, c));
        }
    }
    found.sort_by(|a, b| b.0.total_cmp(&a.0));
    Ok(found.into_iter().map(|(_, c)| c).collect())
}

/// `xcodebuild` for this device. Returns the built .app and the errors/warnings, deduplicated.
pub fn build(d: &Device, scheme: &str, project: Option<&str>) -> R<(Option<PathBuf>, Vec<String>, bool)> {
    let derived = state::state_dir().join("DerivedData");
    let mut cmd = Command::new("xcodebuild");
    if let Some(p) = project {
        cmd.arg(if p.ends_with(".xcworkspace") { "-workspace" } else { "-project" }).arg(p);
    }
    cmd.args(["-scheme", scheme, "-destination", &format!("id={}", d.udid), "-derivedDataPath"]).arg(&derived).arg("build");
    let out = cmd.stdin(Stdio::null()).output().map_err(|e| format!("cannot run xcodebuild: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    let issues = build_issues(&text);
    let products = derived.join("Build/Products");
    let app = std::fs::read_dir(&products)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with("-iphonesimulator"))
        .find_map(|e| std::fs::read_dir(e.path()).ok()?.flatten().map(|a| a.path()).find(|a| a.extension().is_some_and(|x| x == "app")));
    Ok((app, issues, out.status.success()))
}

/// `file:line:col: error: message` lines from xcodebuild output, errors first, without repeats.
pub fn build_issues(text: &str) -> Vec<String> {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    for l in text.lines().map(str::trim) {
        let list = if l.contains(": error: ") || l.starts_with("error: ") {
            &mut errors
        } else if l.contains(": warning: ") {
            &mut warnings
        } else {
            continue;
        };
        if !list.iter().any(|x| x == l) {
            list.push(l.to_string());
        }
    }
    errors.extend(warnings);
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Value {
        json!({ "devices": {
            "com.apple.CoreSimulator.SimRuntime.iOS-26-0": [
                { "udid": "A", "name": "iPhone 17", "state": "Shutdown", "isAvailable": true } ],
            "com.apple.CoreSimulator.SimRuntime.iOS-27-2": [
                { "udid": "B", "name": "iPhone 18 Pro", "state": "Booted", "isAvailable": true },
                { "udid": "C", "name": "iPad Air", "state": "Shutdown", "isAvailable": false } ]
        }})
    }

    #[test]
    fn devices_are_newest_os_first_and_skip_unavailable() {
        let d = parse_devices(&sample());
        assert_eq!(d.iter().map(|d| d.udid.as_str()).collect::<Vec<_>>(), ["B", "A"]);
        assert_eq!(d[0].os, "iOS 27.2");
        assert!(d[0].booted);
    }

    #[test]
    fn device_specific_runtimes_read_as_plain_versions() {
        assert_eq!(os_name("com.apple.CoreSimulator.SimRuntime.iOS-27-2-iPhone19-4"), "iOS 27.2");
    }

    #[test]
    fn find_matches_name_prefix_case_insensitively() {
        let d = parse_devices(&sample());
        assert_eq!(find(&d, "iphone 18").map(|d| d.udid.as_str()), Some("B"));
        assert_eq!(find(&d, "A").map(|d| d.udid.as_str()), Some("A"));
    }

    #[test]
    fn log_line_is_short_and_tagged() {
        let v = json!({ "timestamp": "2026-11-03 21:04:31.123456+1100", "messageType": "Error",
            "subsystem": "com.example", "category": "auth", "eventMessage": "invalid URL\n" });
        assert_eq!(log_line(&v), "21:04:31.123 error [com.example:auth] invalid URL");
    }

    #[test]
    fn build_issues_lists_errors_before_warnings_once() {
        let text = "/a.swift:3:1: warning: unused\n/b.swift:42:9: error: no member 'x'\n/b.swift:42:9: error: no member 'x'\nCompiling...";
        assert_eq!(build_issues(text), ["/b.swift:42:9: error: no member 'x'", "/a.swift:3:1: warning: unused"]);
    }

    #[test]
    fn crash_summary_names_exception_and_frames() {
        let head = json!({ "app_name": "MyApp", "bundleID": "com.example.MyApp", "timestamp": "2026-11-03 21:00:00.00 +1100" });
        let body = json!({
            "exception": { "type": "EXC_BREAKPOINT", "signal": "SIGTRAP" },
            "asi": { "libswiftCore.dylib": ["Fatal error: Index out of range"] },
            "faultingThread": 0,
            "threads": [{ "frames": [
                { "imageIndex": 0, "symbol": "AuthService.login()", "sourceFile": "AuthService.swift", "sourceLine": 42 },
                { "imageIndex": 1, "symbol": "main" } ] }],
            "usedImages": [{ "name": "MyApp" }, { "name": "dyld" }]
        });
        let c = parse_crash(&format!("{head}\n{body}")).unwrap();
        assert_eq!(c["exception"], "EXC_BREAKPOINT (SIGTRAP)");
        assert_eq!(c["reason"], "Fatal error: Index out of range");
        assert_eq!(c["frames"][0], "MyApp  AuthService.login()  AuthService.swift:42");
        assert_eq!(c["frames"][1], "dyld  main");
    }
}
