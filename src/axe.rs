//! Touch, keyboard, the accessibility tree and the video stream through AXe
//! (https://github.com/cameroncooke/AXe), since `simctl` has no input commands.

use crate::R;
use crate::state;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::process::{Child, Command, Stdio};

const INSTALL: &str = "AXe is not installed. Install it with: brew install cameroncooke/axe/axe";

fn cmd(args: &[&str], udid: &str) -> Command {
    let mut c = Command::new("axe");
    c.args(args).args(["--udid", udid]);
    c
}

fn run(args: &[&str], udid: &str) -> R<String> {
    let out = cmd(args, udid)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { INSTALL.to_string() } else { format!("cannot run axe: {e}") })?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("axe {}: {}", args.first().unwrap_or(&""), err.trim()).into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn tap(udid: &str, x: f64, y: f64) -> R<()> {
    run(&["tap", "-x", &format!("{x:.1}"), "-y", &format!("{y:.1}")], udid).map(drop)
}

pub fn swipe(udid: &str, from: (f64, f64), to: (f64, f64), secs: f64) -> R<()> {
    let n = |v: f64| format!("{v:.1}");
    run(&["swipe", "--start-x", &n(from.0), "--start-y", &n(from.1), "--end-x", &n(to.0), "--end-y", &n(to.1), "--duration", &n(secs)], udid)
        .map(drop)
}

pub fn type_text(udid: &str, text: &str) -> R<()> {
    run(&["type", text], udid).map(drop)
}

/// HID keycodes AXe takes for `key`.
pub const KEY_RETURN: u8 = 40;
pub const KEY_BACKSPACE: u8 = 42;
pub const KEY_TAB: u8 = 43;

pub fn key(udid: &str, code: u8) -> R<()> {
    run(&["key", &code.to_string()], udid).map(drop)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Button {
    Home,
    Lock,
    #[value(name = "side-button")]
    Side,
    Siri,
}

pub fn button(udid: &str, b: Button) -> R<()> {
    let name = match b {
        Button::Home => "home",
        Button::Lock => "lock",
        Button::Side => "side-button",
        Button::Siri => "siri",
    };
    run(&["button", name], udid).map(drop)
}

/// Start `axe stream-video --format raw`: each frame is a 4-byte big-endian length then JPEG bytes.
pub fn stream(udid: &str, fps: u32, scale: f64) -> R<Child> {
    cmd(&["stream-video", "--format", "raw", "--fps", &fps.clamp(1, 30).to_string(), "--scale", &format!("{scale:.2}")], udid)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { INSTALL.into() } else { format!("cannot run axe: {e}").into() })
}

/// One on-screen element from the accessibility tree, in device points.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Element {
    pub kind: String,
    pub label: String,
    pub value: String,
    pub id: String,
    pub frame: [f64; 4],
}

impl Element {
    pub fn center(&self) -> (f64, f64) {
        (self.frame[0] + self.frame[2] / 2.0, self.frame[1] + self.frame[3] / 2.0)
    }

    pub fn line(&self, n: usize) -> String {
        let mut s = format!("[{n}] {}", self.kind);
        if !self.label.is_empty() {
            s += &format!(" {:?}", self.label);
        }
        if !self.value.is_empty() && self.value != self.label {
            s += &format!(" value={:?}", self.value);
        }
        if !self.id.is_empty() {
            s += &format!(" id={}", self.id);
        }
        s
    }
}

fn text(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or("").trim().to_string()
}

fn frame(v: &Value) -> Option<[f64; 4]> {
    let f = &v["frame"];
    Some([f["x"].as_f64()?, f["y"].as_f64()?, f["width"].as_f64()?, f["height"].as_f64()?])
}

/// Flatten `describe-ui` JSON to the on-screen elements worth naming (labelled, valued or
/// identified), plus the screen size in points from the root element. Elements whose centre is
/// off screen (other home-screen pages, scrolled-away rows) can't be tapped, so they're left out.
pub fn parse_tree(v: &Value) -> (Vec<Element>, Option<(f64, f64)>) {
    fn walk(v: &Value, out: &mut Vec<Element>) {
        for node in v.as_array().into_iter().flatten() {
            let kind = text(node, "type");
            let (label, value, id) = (text(node, "AXLabel"), text(node, "AXValue"), text(node, "AXUniqueId"));
            if kind != "Application"
                && !(label.is_empty() && value.is_empty() && id.is_empty())
                && let Some(frame) = frame(node).filter(|f| f[2] > 0.0 && f[3] > 0.0)
            {
                out.push(Element { kind: if kind.is_empty() { "Element".into() } else { kind }, label, value, id, frame });
            }
            walk(&node["children"], out);
        }
    }
    let mut out = Vec::new();
    walk(v, &mut out);
    let screen = v.as_array().and_then(|a| a.first()).and_then(frame).map(|f| (f[2], f[3]));
    if let Some((w, h)) = screen {
        out.retain(|e| {
            let (x, y) = e.center();
            (0.0..=w).contains(&x) && (0.0..=h).contains(&y)
        });
    }
    (out, screen)
}

pub fn describe_raw(udid: &str) -> R<Value> {
    Ok(serde_json::from_str(&run(&["describe-ui"], udid)?)?)
}

/// Describe the screen and remember the numbering so `tap N` can use it.
pub fn describe(udid: &str) -> R<Vec<Element>> {
    let (els, _) = parse_tree(&describe_raw(udid)?);
    let dir = state::device_dir(udid);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("elements.json"), serde_json::to_string(&els)?)?;
    Ok(els)
}

/// Element `n` (1-based) from the last `ui`.
pub fn element(udid: &str, n: usize) -> R<Element> {
    let s = std::fs::read_to_string(state::device_dir(udid).join("elements.json")).map_err(|_| "run `herdr-ios ui` first")?;
    let els: Vec<Element> = serde_json::from_str(&s)?;
    n.checked_sub(1).and_then(|i| els.get(i)).cloned().ok_or_else(|| format!("no element [{n}]; run `herdr-ios ui` again").into())
}

/// Screen size in points, from the accessibility tree's root.
pub fn screen_points(udid: &str) -> R<(f64, f64)> {
    parse_tree(&describe_raw(udid)?).1.ok_or_else(|| "cannot read the screen size from axe describe-ui".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tree() -> Value {
        json!([{ "type": "Application", "AXLabel": "MyApp", "frame": { "x": 0, "y": 0, "width": 402, "height": 874 },
            "children": [
                { "type": "StaticText", "AXLabel": "Welcome", "frame": { "x": 150, "y": 100, "width": 100, "height": 30 }, "children": [] },
                { "type": "Button", "AXLabel": "Maps", "frame": { "x": -377, "y": 88, "width": 168, "height": 191 }, "children": [] },
                { "type": "Group", "AXLabel": "", "frame": { "x": 0, "y": 0, "width": 402, "height": 400 }, "children": [
                    { "type": "Button", "AXLabel": "Sign in", "AXUniqueId": "signIn", "frame": { "x": 100, "y": 300, "width": 200, "height": 44 } } ] }
            ] }])
    }

    #[test]
    fn tree_flattens_to_named_elements() {
        let (els, _) = parse_tree(&tree());
        assert_eq!(els.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(), ["Welcome", "Sign in"]);
    }

    #[test]
    fn screen_size_comes_from_the_root() {
        assert_eq!(parse_tree(&tree()).1, Some((402.0, 874.0)));
    }

    #[test]
    fn element_line_and_center() {
        let (els, _) = parse_tree(&tree());
        assert_eq!(els[1].line(2), "[2] Button \"Sign in\" id=signIn");
        assert_eq!(els[1].center(), (200.0, 322.0));
    }
}
