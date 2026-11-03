//! The simulator in a terminal pane: AXe video frames drawn by `render`, a header with
//! buttons, clicks and drags sent as taps and swipes, and typing sent as keyboard input.

use crate::agent::{self, SendKind};
use crate::icons::{self, Icon, Rgb};
use crate::render::{self, Cells, IMAGE_ID, Renderer, SYNC_OFF, SYNC_ON};
use crate::sim::{self, Device};
use crate::state::{self, Config};
use crate::{R, axe};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use crossterm::{cursor, execute, queue, terminal};
use std::io::{Read, Write, stdout};
use std::process::Child;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

/// The device image starts below the one-line header.
const TOP: u16 = 1;
/// Device points scrolled per wheel tick, and the pause that ends a scroll.
const WHEEL_STEP: f64 = 40.0;
const WHEEL_IDLE: Duration = Duration::from_millis(150);

type Input = Box<dyn FnOnce(&str) -> R<()> + Send>;

/// Answer the agent's pick with the element under `p`. Numbered as `ui` numbers them (and
/// saved as the last `ui`), so the agent can `tap n` straight away.
fn answer_pick(udid: &str, p: (f64, f64)) -> R<()> {
    let els = axe::describe(udid)?;
    let i = axe::element_at(&els, p).ok_or("nothing to pick there; click an element")?;
    let line = els[i].line(i + 1);
    let answer = serde_json::json!({ "n": i + 1, "line": line, "element": els[i] });
    state::update(|s| s.pick.iter_mut().for_each(|pk| pk.answer = Some(answer.clone())))?;
    state::event(udid, "user pick", &line)
}

/// The thread that runs input for the device, in order; errors go to the header.
fn input_thread(udid: String, tx: Sender<Msg>) -> Sender<Input> {
    let (send, recv) = mpsc::channel::<Input>();
    std::thread::spawn(move || {
        for f in recv {
            if let Err(e) = f(&udid) {
                let _ = tx.send(Msg::Say(format!("error: {e}")));
            }
        }
    });
    send
}

/// Frames and stream ends carry the stream generation, so a replaced stream's last frames are dropped.
enum Msg {
    Frame(u64, usize, usize, Vec<u8>),
    Gone(u64, String),
    Say(String),
}

/// Read `axe stream-video --format raw` frames (4-byte big-endian length + JPEG) as RGB.
fn read_frames(generation: u64, mut child: Child, tx: Sender<Msg>) {
    let Some(mut out) = child.stdout.take() else { return };
    let err = loop {
        let mut len = [0u8; 4];
        if let Err(e) = out.read_exact(&mut len) {
            break format!("video stream ended ({e})");
        }
        let mut jpeg = vec![0u8; u32::from_be_bytes(len) as usize];
        if let Err(e) = out.read_exact(&mut jpeg) {
            break format!("video stream ended ({e})");
        }
        let mut dec = zune_jpeg::JpegDecoder::new(std::io::Cursor::new(&jpeg));
        let Ok(rgb) = dec.decode() else { continue };
        let Some((w, h)) = dec.dimensions() else { continue };
        if tx.send(Msg::Frame(generation, w, h, rgb)).is_err() {
            break String::new();
        }
    };
    let _ = child.kill();
    let _ = child.wait();
    let _ = tx.send(Msg::Gone(generation, err));
}

struct Term;

impl Term {
    fn enter() -> R<Term> {
        terminal::enable_raw_mode()?;
        execute!(stdout(), terminal::EnterAlternateScreen, cursor::Hide, event::EnableMouseCapture, terminal::SetTitle("iOS Simulator"))?;
        Ok(Term)
    }
}

impl Drop for Term {
    fn drop(&mut self) {
        let mut out = stdout();
        render::kitty_clear(&mut out);
        for id in icon_ids() {
            let _ = write!(out, "\x1b_Ga=d,d=I,i={id},q=2\x1b\\");
        }
        let _ = execute!(out, event::DisableMouseCapture, cursor::Show, terminal::LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Act {
    Home,
    Lock,
    SendShot,
    SendErrors,
    Quit,
}

impl Act {
    /// Shown in the header while the button is hovered.
    fn describe(self) -> &'static str {
        match self {
            Act::Home => "Home (Ctrl-O)",
            Act::Lock => "Lock",
            Act::SendShot => "Send screenshot to agent (Ctrl-X)",
            Act::SendErrors => "Send the app's recent errors to agent",
            Act::Quit => "Close view (Ctrl-Q)",
        }
    }
}

/// Header buttons, right-aligned; `None` is a gap between groups.
const BUTTONS: [Option<(Icon, Act)>; 7] = [
    Some((Icon::Home, Act::Home)),
    Some((Icon::Lock, Act::Lock)),
    None,
    Some((Icon::Camera, Act::SendShot)),
    Some((Icon::Alert, Act::SendErrors)),
    None,
    Some((Icon::Close, Act::Quit)),
];
const BUTTON_COLS: u16 = 4;

/// herdr desktop's default dark look (from herdr-web-browser).
// ponytail: fixed palette; sync herdr's theme like the browser's theme mod if light mode matters.
const BAR: Rgb = (33, 37, 43);
const HOVER: Rgb = (53, 57, 62);
const FG: Rgb = (238, 238, 239);
const MUTED: Rgb = (156, 158, 161);
const DIM: Rgb = (111, 113, 117);
const OK: Rgb = (97, 175, 239);
const ERR: Rgb = (224, 108, 117);

const ICON_BASE: u32 = 6000;
const PILL_BASE: u32 = 6100;

fn icon_ids() -> impl Iterator<Item = u32> {
    (0..icons::ALL.len() as u32).flat_map(|i| [ICON_BASE + i, PILL_BASE + i])
}

/// An icon image in the header: (col, image id, width in cols).
type Slot = (u16, u32, u16);

/// A clickable header span.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Hit {
    x0: u16,
    x1: u16,
    act: Act,
}

fn paint(out: &mut Vec<u8>, s: &str, fg: Rgb, bg: Rgb) {
    let _ = write!(out, "\x1b[38;2;{};{};{}m\x1b[48;2;{};{};{}m{s}", fg.0, fg.1, fg.2, bg.0, bg.1, bg.2);
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let mut t: String = s.chars().take(n.saturating_sub(1)).collect();
    t.push('…');
    t
}

/// The header row for a `cols`-wide pane: device, status text, then the buttons on the right.
/// Returns the bytes, the button spans and the icon slots (col, image id, width in cols).
fn header_line(cols: u16, title: (&str, &str), status: (&str, Rgb), hover: Option<u16>, images: bool) -> (Vec<u8>, Vec<Hit>, Vec<Slot>) {
    let mut out = Vec::new();
    let _ = queue!(out, cursor::MoveTo(0, 0));
    let buttons_w = BUTTONS.iter().map(|b| if b.is_some() { BUTTON_COLS } else { 1 }).sum::<u16>() + 1;
    let left = format!(" {} ", title.0);
    let os = format!("· {}  ", title.1);
    let room = (cols.saturating_sub(buttons_w) as usize).saturating_sub(left.chars().count() + os.chars().count());
    let text = clip(status.0, room);
    paint(&mut out, &left, FG, BAR);
    paint(&mut out, &os, DIM, BAR);
    paint(&mut out, &format!("{text}{}", " ".repeat(room - text.chars().count())), status.1, BAR);
    let mut col = cols.saturating_sub(buttons_w);
    let (mut hits, mut slots) = (Vec::new(), Vec::new());
    for b in BUTTONS {
        let Some((icon, act)) = b else {
            paint(&mut out, " ", FG, BAR);
            col += 1;
            continue;
        };
        let hot = hover.is_some_and(|c| (col..col + BUTTON_COLS).contains(&c));
        let i = icons::ALL.iter().position(|x| *x == icon).unwrap_or(0) as u32;
        if images {
            slots.push(if hot { (col, PILL_BASE + i, BUTTON_COLS) } else { (col + 1, ICON_BASE + i, 2) });
            paint(&mut out, "    ", FG, BAR);
        } else {
            let (fg, bg) = if hot { (FG, HOVER) } else { (MUTED, BAR) };
            paint(&mut out, &format!(" {}  ", icon.glyph()), fg, bg);
        }
        hits.push(Hit { x0: col, x1: col + BUTTON_COLS, act });
        col += BUTTON_COLS;
    }
    paint(&mut out, " ", FG, BAR);
    out.extend_from_slice(b"\x1b[0m");
    (out, hits, slots)
}

/// Where the device image sits in the pane, in cells.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Placed {
    left: u16,
    cols: u16,
    rows: u16,
}

impl Placed {
    /// Pane cell -> device point, if the cell is on the image.
    fn points_at(&self, col: u16, row: u16, screen: (f64, f64)) -> Option<(f64, f64)> {
        let (x, y) = (col.checked_sub(self.left)?, row.checked_sub(TOP)?);
        (x < self.cols && y < self.rows).then(|| ((x as f64 + 0.5) / self.cols as f64 * screen.0, (y as f64 + 0.5) / self.rows as f64 * screen.1))
    }
}

/// Stream scale that makes frames about `want_w` px wide, given a `got_w` px frame at `scale`.
fn rescale(scale: f64, got_w: usize, want_w: f64) -> f64 {
    (scale * want_w / got_w as f64).clamp(0.1, 1.0)
}

struct View {
    cfg: Config,
    dev: Device,
    cells: Cells,
    renderer: Renderer,
    tx: Sender<Msg>,
    /// Running stream: its scale and pid.
    stream: Option<(f64, u32)>,
    generation: u64,
    scale: f64,
    screen: Option<(f64, f64)>,
    placed: Option<Placed>,
    flip: bool,
    message: String,
    message_at: Instant,
    press: Option<(u16, u16, Instant)>,
    /// Wheel scrolling not yet sent: start point, distance and when it last moved.
    wheel: Option<(f64, f64, f64, Instant)>,
    inputs: Sender<Input>,
    typed: String,
    /// Header column under the mouse.
    hover: Option<u16>,
    /// An agent's `pick` prompt: the next click picks an element instead of tapping.
    pick: Option<String>,
    hits: Vec<Hit>,
    /// Header bytes as last written; redraw only on change.
    drawn: Vec<u8>,
}

impl View {
    fn say(&mut self, m: impl Into<String>) {
        self.message = m.into();
        self.message_at = Instant::now();
    }

    fn ensure_stream(&mut self) -> R<()> {
        if self.stream.is_none() {
            let child = axe::stream(&self.dev.udid, self.cfg.fps.unwrap_or(15), self.scale)?;
            let (tx, generation) = (self.tx.clone(), self.generation);
            self.stream = Some((self.scale, child.id()));
            std::thread::spawn(move || read_frames(generation, child, tx));
        }
        if self.screen.is_none() {
            self.screen = Some(axe::screen_points(&self.dev.udid)?);
        }
        Ok(())
    }

    /// Queue `f` for the input thread, which runs one at a time in order: each axe call takes
    /// ~1s to start, and overlapping touches cancel each other. Failures show in the header.
    fn input(&self, f: impl FnOnce(&str) -> R<()> + Send + 'static) {
        let _ = self.inputs.send(Box::new(f));
    }

    /// Send the scrolling gathered since the wheel last moved as one drag.
    fn flush_wheel(&mut self) {
        let Some((x, y, dy, _)) = self.wheel.take() else { return };
        let Some(screen) = self.screen else { return };
        let to = (x, (y + dy).clamp(1.0, screen.1 - 1.0));
        self.input(move |u| axe::drag(u, (x, y), to, 0.5));
    }

    /// The header's status text: a hovered button's name, else a recent message.
    fn status(&self) -> (String, Rgb) {
        if let Some(h) = self.hover.and_then(|c| self.hits.iter().find(|h| (h.x0..h.x1).contains(&c))) {
            return (h.act.describe().into(), FG);
        }
        if let Some(p) = &self.pick {
            return (format!("{p} · click it · Esc cancels"), OK);
        }
        if self.message_at.elapsed() < Duration::from_secs(6) {
            return (self.message.clone(), if self.message.starts_with("error") { ERR } else { OK });
        }
        (String::new(), DIM)
    }

    /// Upload the icons (plain and hovered pill) as Kitty images; again after a resize.
    fn upload_icons(&mut self) -> R<()> {
        if self.renderer != Renderer::Kitty {
            return Ok(());
        }
        use base64::Engine;
        let b64 = |png: Vec<u8>| base64::engine::general_purpose::STANDARD.encode(png);
        let (cw, h) = ((self.cells.w * 2.0).round() as u32, (self.cells.h * 2.0).round() as u32);
        let mut out = Vec::new();
        for (i, icon) in icons::ALL.iter().enumerate() {
            render::kitty_upload(&mut out, &b64(icon.png(MUTED, cw * 2, h, 0.62)?), ICON_BASE + i as u32)?;
            render::kitty_upload(&mut out, &b64(icons::pill_png(*icon, FG, HOVER, cw * BUTTON_COLS as u32, h)?), PILL_BASE + i as u32)?;
        }
        let mut so = stdout().lock();
        so.write_all(&out)?;
        so.flush()?;
        self.drawn.clear();
        Ok(())
    }

    /// Kill the stream; its reader sees EOF and exits, and the new generation ignores it.
    fn stop_stream(&mut self) {
        if let Some((_, pid)) = self.stream.take() {
            // SAFETY: plain kill(2) on our own child's pid.
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
        }
        self.generation += 1;
    }

    fn draw_header(&mut self) -> R<()> {
        let (cols, _) = terminal::size()?;
        let (text, color) = self.status();
        let title = (self.dev.name.as_str(), self.dev.os.as_str());
        let (line, hits, slots) = header_line(cols, title, (&text, color), self.hover, self.renderer == Renderer::Kitty);
        self.hits = hits;
        let mut out = Vec::from(SYNC_ON.as_bytes());
        out.extend_from_slice(&line);
        if !slots.is_empty() {
            // Re-place every icon: drop old placements (keeping the images), put new ones.
            for id in icon_ids() {
                write!(out, "\x1b_Ga=d,d=i,i={id},q=2\x1b\\")?;
            }
            for (col, id, w) in slots {
                queue!(out, cursor::MoveTo(col, 0))?;
                write!(out, "\x1b_Ga=p,i={id},p=1,c={},r=1,C=1,q=2\x1b\\", self.cells.img_cols(w))?;
            }
        }
        out.extend_from_slice(SYNC_OFF.as_bytes());
        if out != self.drawn {
            let mut so = stdout().lock();
            so.write_all(&out)?;
            so.flush()?;
            self.drawn = out;
        }
        Ok(())
    }

    fn draw_frame(&mut self, w: usize, h: usize, rgb: &[u8]) -> R<()> {
        let (cols, rows) = terminal::size()?;
        let rows = rows.saturating_sub(TOP).max(1);
        let (c, r) = render::fit(w as f64, h as f64, cols, rows, self.cells.aspect());
        let placed = Placed { left: (cols - c) / 2, cols: c, rows: r };
        let want_w = match self.renderer {
            Renderer::Kitty => c as f64 * self.cells.w,
            Renderer::Blocks => c as f64,
        };
        // Frames far off the size drawn waste decode time or look soft: restart at a better scale.
        if self.stream.is_some_and(|(s, _)| s == self.scale) && ((w as f64) > want_w * 1.3 || ((w as f64) < want_w * 0.7 && self.scale < 1.0)) {
            let s = rescale(self.scale, w, want_w);
            if (s - self.scale).abs() > 0.02 {
                self.scale = s;
            }
        }
        let mut out = Vec::from(SYNC_ON.as_bytes());
        if self.placed != Some(placed) {
            render::kitty_clear(&mut out);
            for row in TOP..TOP + rows {
                queue!(out, cursor::MoveTo(0, row), terminal::Clear(terminal::ClearType::CurrentLine))?;
            }
        }
        match self.renderer {
            Renderer::Kitty => {
                let (id, old) = if self.flip { (IMAGE_ID + 1, IMAGE_ID) } else { (IMAGE_ID, IMAGE_ID + 1) };
                self.flip = !self.flip;
                queue!(out, cursor::MoveTo(placed.left, TOP))?;
                render::kitty_swap(&mut out, &render::png_b64(w, h, rgb)?, id, old, self.cells.img_cols(c), r)?;
            }
            Renderer::Blocks => render::blocks(&mut out, (w, h, rgb), TOP, placed.left, c, r)?,
        }
        out.extend_from_slice(SYNC_OFF.as_bytes());
        self.placed = Some(placed);
        let mut so = stdout().lock();
        so.write_all(&out)?;
        so.flush()?;
        Ok(())
    }

    fn act(&mut self, act: Act) -> bool {
        match act {
            Act::Home => self.input(|u| axe::button(u, axe::Button::Home)),
            Act::Lock => self.input(|u| axe::button(u, axe::Button::Lock)),
            Act::SendShot => self.send(SendKind::Screenshot),
            Act::SendErrors => self.send(SendKind::Errors),
            Act::Quit => return false,
        }
        true
    }

    /// Send to the agent off the UI thread; the result shows in the header.
    fn send(&mut self, kind: SendKind) {
        self.say("sending…");
        let (dev, tx) = (self.dev.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let msg = match agent::send(&dev, kind, None) {
                Ok(to) => format!("sent to {to}"),
                Err(e) => format!("error: {e}"),
            };
            let _ = tx.send(Msg::Say(msg));
        });
    }

    fn flush_typing(&mut self) {
        if !self.typed.is_empty() {
            let text = std::mem::take(&mut self.typed);
            self.input(move |u| axe::type_text(u, &text));
        }
    }

    fn on_key(&mut self, k: KeyEvent) -> bool {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Char('q') if ctrl => return false,
            KeyCode::Esc if self.pick.take().is_some() => {
                self.say("pick cancelled");
                self.input(|_| state::update(|s| s.pick.iter_mut().for_each(|p| p.answer = Some(serde_json::json!({ "cancelled": true })))));
            }
            KeyCode::Char('o') if ctrl => return self.act(Act::Home),
            KeyCode::Char('x') if ctrl => return self.act(Act::SendShot),
            KeyCode::Char(c) if !ctrl => self.typed.push(c),
            KeyCode::Enter | KeyCode::Backspace | KeyCode::Tab => {
                self.flush_typing();
                let code = match k.code {
                    KeyCode::Enter => axe::KEY_RETURN,
                    KeyCode::Backspace => axe::KEY_BACKSPACE,
                    _ => axe::KEY_TAB,
                };
                self.input(move |u| axe::key(u, code));
            }
            _ => {}
        }
        true
    }

    fn on_mouse(&mut self, m: event::MouseEvent) -> bool {
        if matches!(m.kind, MouseEventKind::Moved | MouseEventKind::Drag(_)) {
            self.hover = (m.row < TOP).then_some(m.column);
        }
        let (Some(placed), Some(screen)) = (self.placed, self.screen) else { return true };
        let at = placed.points_at(m.column, m.row, screen);
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) if m.row < TOP => {
                if let Some(h) = self.hits.iter().find(|h| (h.x0..h.x1).contains(&m.column)) {
                    return self.act(h.act);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => self.press = at.is_some().then_some((m.column, m.row, Instant::now())),
            MouseEventKind::Up(MouseButton::Left) => {
                let Some((c0, r0, t0)) = self.press.take() else { return true };
                let Some(from) = placed.points_at(c0, r0, screen) else { return true };
                let to = placed
                    .points_at(m.column.clamp(placed.left, placed.left + placed.cols - 1), m.row.clamp(TOP, TOP + placed.rows - 1), screen)
                    .unwrap_or(from);
                let udid = self.dev.udid.clone();
                if c0.abs_diff(m.column) + r0.abs_diff(m.row) <= 1 && self.pick.take().is_some() {
                    self.say("picked");
                    self.input(move |u| answer_pick(u, from));
                } else if c0.abs_diff(m.column) + r0.abs_diff(m.row) <= 1 {
                    let _ = state::event(&udid, "user tap", &format!("{:.0},{:.0}", from.0, from.1));
                    self.input(move |u| axe::tap(u, from.0, from.1));
                } else {
                    let _ = state::event(&udid, "user swipe", &format!("{:.0},{:.0} -> {:.0},{:.0}", from.0, from.1, to.0, to.1));
                    // Replay at the speed it was dragged: slow drags scroll by their length, flicks fling.
                    let secs = t0.elapsed().as_secs_f64().clamp(0.1, 2.0);
                    self.input(move |u| axe::drag(u, from, to, secs));
                }
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                if let Some((x, y)) = at {
                    let step = if m.kind == MouseEventKind::ScrollDown { -WHEEL_STEP } else { WHEEL_STEP };
                    let (x, y, dy, _) = self.wheel.unwrap_or((x, y, 0.0, Instant::now()));
                    let cap = screen.1 * 0.6;
                    self.wheel = Some((x, y, (dy + step).clamp(-cap, cap), Instant::now()));
                }
            }
            _ => {}
        }
        true
    }
}

pub fn run(cfg: Config, device: Option<&str>) -> R<()> {
    let _term = Term::enter()?;
    let mut so = stdout();
    write!(so, "\x1b[1;1H booting simulator…")?;
    so.flush()?;
    let dev = sim::boot(&cfg, device)?;
    let (cells, renderer) = render::detect(&cfg);
    let (tx, rx) = mpsc::channel();
    let (udid, tx2) = (dev.udid.clone(), tx.clone());
    let mut v = View {
        cfg,
        dev,
        cells,
        renderer,
        tx,
        stream: None,
        generation: 0,
        scale: 0.5,
        screen: None,
        placed: None,
        flip: false,
        message: String::new(),
        message_at: Instant::now() - Duration::from_secs(60),
        press: None,
        wheel: None,
        inputs: input_thread(udid, tx2),
        typed: String::new(),
        hover: None,
        pick: None,
        hits: Vec::new(),
        drawn: Vec::new(),
    };
    v.upload_icons()?;
    let result = event_loop(&mut v, &rx);
    v.stop_stream();
    result
}

fn event_loop(v: &mut View, rx: &Receiver<Msg>) -> R<()> {
    let mut last_tick = Instant::now() - Duration::from_secs(10);
    loop {
        if last_tick.elapsed() >= Duration::from_secs(1) {
            last_tick = Instant::now();
            // The agent's pick request, if any, shows within a second.
            v.pick = state::load().pick.filter(|p| p.answer.is_none()).map(|p| p.prompt);
            if let Err(e) = v.ensure_stream() {
                v.say(format!("error: {e}"));
            }
            v.draw_header()?;
        }
        let mut latest = None;
        while let Ok(m) = rx.try_recv() {
            match m {
                Msg::Frame(g, w, h, rgb) if g == v.generation => latest = Some((w, h, rgb)),
                Msg::Frame(..) => {}
                Msg::Gone(g, e) if g == v.generation => {
                    v.stream = None;
                    v.generation += 1;
                    if !e.is_empty() {
                        v.say(e);
                    }
                }
                Msg::Gone(..) => {}
                Msg::Say(s) => v.say(s),
            }
        }
        if let Some((w, h, rgb)) = latest {
            if let Err(e) = v.draw_frame(w, h, &rgb) {
                v.say(format!("error: {e}"));
            }
            // draw_frame picked a new scale: restart the stream at it on the next tick.
            if v.stream.is_some_and(|(s, _)| s != v.scale) {
                v.stop_stream();
                last_tick = Instant::now() - Duration::from_secs(10);
            }
        }
        if !event::poll(Duration::from_millis(30))? {
            v.flush_typing();
            if v.wheel.is_some_and(|w| w.3.elapsed() > WHEEL_IDLE) {
                v.flush_wheel();
            }
            continue;
        }
        loop {
            let keep = match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => v.on_key(k),
                Event::Mouse(m) => v.on_mouse(m),
                Event::Resize(..) => {
                    v.placed = None;
                    v.upload_icons()?;
                    last_tick = Instant::now() - Duration::from_secs(10);
                    true
                }
                _ => true,
            };
            if !keep {
                return Ok(());
            }
            if !event::poll(Duration::ZERO)? {
                break;
            }
        }
        v.flush_typing();
        v.draw_header()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_map_to_device_points() {
        let p = Placed { left: 10, cols: 32, rows: 64 };
        // Cell (16, 32) of a 32x64 image on a 400x800 pt screen: its centre.
        assert_eq!(p.points_at(26, TOP + 32, (400.0, 800.0)), Some((206.25, 406.25)));
    }

    #[test]
    fn cells_off_the_image_are_not_points() {
        let p = Placed { left: 10, cols: 40, rows: 80 };
        assert_eq!(p.points_at(9, TOP, (400.0, 800.0)), None);
        assert_eq!(p.points_at(50, TOP, (400.0, 800.0)), None);
        assert_eq!(p.points_at(20, 0, (400.0, 800.0)), None);
    }

    #[test]
    fn header_buttons_sit_right_and_hover_shows_a_pill() {
        let (_, hits, slots) = header_line(80, ("iPhone 18 Pro", "iOS 27.2"), ("", DIM), Some(77), true);
        let acts: Vec<Act> = hits.iter().map(|h| h.act).collect();
        assert_eq!(acts, [Act::Home, Act::Lock, Act::SendShot, Act::SendErrors, Act::Quit]);
        assert_eq!(hits.last().map(|h| (h.x0, h.x1)), Some((79 - BUTTON_COLS, 79)));
        // Hovering the close button places its pill over all 4 cells; the others are bare icons.
        assert_eq!(slots.last(), Some(&(79 - BUTTON_COLS, PILL_BASE + 4, BUTTON_COLS)));
        assert_eq!(slots[0], (hits[0].x0 + 1, ICON_BASE, 2));
    }

    #[test]
    fn rescale_targets_the_drawn_width() {
        assert_eq!(rescale(0.5, 600, 300.0), 0.25);
        assert_eq!(rescale(0.5, 100, 1000.0), 1.0);
        assert_eq!(rescale(0.5, 6000, 10.0), 0.1);
    }
}
