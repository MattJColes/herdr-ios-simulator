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

// AXe's default tap and an un-held button send down and up together, which the simulator
// drops on Xcode 27 (axe reports success, nothing happens); a real touch lands, and Home needs a
// 0.2s hold (0.1s works only sometimes; Siri needs much longer).
pub fn tap(udid: &str, x: f64, y: f64) -> R<()> {
    run(&["tap", "-x", &format!("{x:.1}"), "-y", &format!("{y:.1}"), "--tap-style", "physical"], udid).map(drop)
}

/// A finger drag with explicit touch moves over `secs`: scrolls by about its length when
/// slow, and flings when quick, like a real touch. (`axe swipe` flings even when slow.)
pub fn drag(udid: &str, from: (f64, f64), to: (f64, f64), secs: f64) -> R<()> {
    let n = |v: f64| format!("{v:.1}");
    let steps = ((secs * 60.0) as u32).clamp(10, 120).to_string();
    let args = [
        "drag",
        "--start-x",
        &n(from.0),
        "--start-y",
        &n(from.1),
        "--end-x",
        &n(to.0),
        "--end-y",
        &n(to.1),
        "--duration",
        &n(secs),
        "--steps",
        &steps,
    ];
    run(&args, udid).map(drop)
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
    run(&["button", name, "--duration", "0.2"], udid).map(drop)
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
        let q = |s: &str| format!("\"{}\"", s.replace('"', "\\\""));
        let mut s = format!("[{n}] {}", self.kind);
        if !self.label.is_empty() {
            s += &format!(" {}", q(&self.label));
        }
        if !self.value.is_empty() && self.value != self.label {
            s += &format!(" value={}", q(&self.value));
        }
        if !self.id.is_empty() {
            s += &format!(" id={}", self.id);
        }
        s
    }
}

/// A label with invisible formatting marks dropped and odd spaces made plain, so `ui` prints
/// `"9:55 pm"` rather than `"9:55\u{202f}pm"`.
fn text(v: &Value, k: &str) -> String {
    let s = v[k].as_str().unwrap_or("");
    s.chars()
        .filter(|c| !matches!(c, '\u{200b}'..='\u{200f}' | '\u{2060}' | '\u{feff}'))
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Worth listing: has a label or value, or is a control with an id. Unlabelled containers
/// (Safari's `Group id=TabDocument?...`) and scroll bars are noise an agent can't act on.
fn worth_naming(kind: &str, label: &str, value: &str, id: &str) -> bool {
    let container = matches!(kind, "Application" | "Group" | "GenericElement" | "Other" | "ScrollView");
    if kind == "Slider" && label.contains("scroll bar") {
        return false;
    }
    !label.is_empty() || (!container && !(value.is_empty() && id.is_empty()))
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
                && worth_naming(&kind, &label, &value, &id)
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

/// Index of the element under `p`: the smallest one containing it, preferring controls over
/// the text and images inside them, so a click on a button's label picks the button.
pub fn element_at(els: &[Element], p: (f64, f64)) -> Option<usize> {
    let inside = |e: &Element| p.0 >= e.frame[0] && p.0 < e.frame[0] + e.frame[2] && p.1 >= e.frame[1] && p.1 < e.frame[1] + e.frame[3];
    let area = |e: &Element| e.frame[2] * e.frame[3];
    let content = |e: &Element| matches!(e.kind.as_str(), "StaticText" | "Image");
    let hits: Vec<usize> = (0..els.len()).filter(|&i| inside(&els[i])).collect();
    let smallest = |only_controls: bool| {
        hits.iter().copied().filter(|&i| !only_controls || !content(&els[i])).min_by(|&a, &b| area(&els[a]).total_cmp(&area(&els[b])))
    };
    smallest(true).or_else(|| smallest(false))
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
    fn unlabelled_containers_and_scroll_bars_are_left_out() {
        let t = json!([{ "type": "Application", "frame": { "x": 0, "y": 0, "width": 402, "height": 874 }, "children": [
            { "type": "Group", "AXUniqueId": "TabDocument?UUID=1", "frame": { "x": 0, "y": 0, "width": 402, "height": 800 } },
            { "type": "Slider", "AXLabel": "Vertical scroll bar, 2 pages", "AXValue": "0%", "frame": { "x": 390, "y": 0, "width": 10, "height": 800 } },
            { "type": "TextField", "AXValue": "Name", "AXUniqueId": "name", "frame": { "x": 10, "y": 100, "width": 300, "height": 40 } },
            { "type": "StaticText", "AXLabel": "9:55\u{202f}pm\u{200e}", "frame": { "x": 10, "y": 10, "width": 60, "height": 20 } } ] }]);
        let lines: Vec<String> = parse_tree(&t).0.iter().enumerate().map(|(i, e)| e.line(i + 1)).collect();
        assert_eq!(lines, ["[1] TextField value=\"Name\" id=name", "[2] StaticText \"9:55 pm\""]);
    }

    #[test]
    fn element_at_prefers_the_control_under_its_label() {
        let el = |kind: &str, frame: [f64; 4]| Element { kind: kind.into(), label: "x".into(), value: String::new(), id: String::new(), frame };
        let els = [el("Group", [0.0, 0.0, 400.0, 800.0]), el("Button", [10.0, 100.0, 300.0, 50.0]), el("StaticText", [20.0, 110.0, 80.0, 20.0])];
        assert_eq!(element_at(&els, (30.0, 120.0)), Some(1));
        assert_eq!(element_at(&els, (350.0, 500.0)), Some(0));
        assert_eq!(element_at(&[els[2].clone()], (30.0, 120.0)), Some(0));
        assert_eq!(element_at(&els, (500.0, 500.0)), None);
    }

    #[test]
    fn element_line_and_center() {
        let (els, _) = parse_tree(&tree());
        assert_eq!(els[1].line(2), "[2] Button \"Sign in\" id=signIn");
        assert_eq!(els[1].center(), (200.0, 322.0));
    }
}
