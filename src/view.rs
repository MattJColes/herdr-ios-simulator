//! The simulator in a terminal pane: AXe video frames drawn by `render`, a header with
//! buttons, clicks and drags sent as taps and swipes, and typing sent as keyboard input.

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
        execute!(stdout(), terminal::EnterAlternateScreen, cursor::Hide, event::EnableMouseCapture)?;
        Ok(Term)
    }
}

impl Drop for Term {
    fn drop(&mut self) {
        let mut out = stdout();
        render::kitty_clear(&mut out);
        let _ = execute!(out, event::DisableMouseCapture, cursor::Show, terminal::LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Act {
    Home,
    Lock,
    Shot,
    Quit,
}

const BUTTONS: [(&str, Act); 4] = [(" Home ", Act::Home), (" Lock ", Act::Lock), (" Screenshot ", Act::Shot), (" Quit ", Act::Quit)];

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
    press: Option<(u16, u16)>,
    typed: String,
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

    /// Run `f` off the UI thread; failures show in the header.
    fn input(&self, f: impl FnOnce(&str) -> R<()> + Send + 'static) {
        let (udid, tx) = (self.dev.udid.clone(), self.tx.clone());
        std::thread::spawn(move || {
            if let Err(e) = f(&udid) {
                let _ = tx.send(Msg::Say(format!("error: {e}")));
            }
        });
    }

    fn header(&self) -> R<Vec<u8>> {
        let (cols, _) = terminal::size()?;
        let mut s = format!(" {} · {} ", self.dev.name, self.dev.os);
        for (label, _) in BUTTONS {
            s += &format!("\x1b[7m{label}\x1b[0m ");
        }
        if self.message_at.elapsed() < Duration::from_secs(6) {
            s += &format!(" {}", self.message);
        }
        let mut out = Vec::new();
        queue!(out, cursor::MoveTo(0, 0), terminal::Clear(terminal::ClearType::CurrentLine))?;
        // Clip by visible width; escapes don't take columns.
        let (mut visible, mut esc) = (0usize, false);
        for ch in s.chars() {
            esc = esc || ch == '\x1b';
            if !esc {
                if visible >= cols as usize {
                    break;
                }
                visible += 1;
            }
            esc = esc && !ch.is_ascii_alphabetic();
            out.extend_from_slice(ch.encode_utf8(&mut [0; 4]).as_bytes());
        }
        out.extend_from_slice(b"\x1b[0m");
        Ok(out)
    }

    fn button_at(&self, col: u16) -> Option<Act> {
        let mut x = format!(" {} · {} ", self.dev.name, self.dev.os).chars().count();
        for (label, act) in BUTTONS {
            let w = label.chars().count();
            if (x..x + w).contains(&(col as usize)) {
                return Some(act);
            }
            x += w + 1;
        }
        None
    }

    /// Kill the stream; its reader sees EOF and exits, and the new generation ignores it.
    fn stop_stream(&mut self) {
        if let Some((_, pid)) = self.stream.take() {
            // SAFETY: plain kill(2) on our own child's pid.
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
        }
        self.generation += 1;
    }

    fn draw_header(&self) -> R<()> {
        let out = self.header()?;
        let mut so = stdout().lock();
        so.write_all(&out)?;
        so.flush()?;
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
            Act::Shot => {
                let dir = state::state_dir().join("screenshots");
                let path = dir.join(format!("{}.png", state::now() as u64));
                match std::fs::create_dir_all(&dir).map_err(Into::into).and_then(|_| sim::screenshot(&self.dev, &path)) {
                    Ok(()) => self.say(format!("saved {}", path.display())),
                    Err(e) => self.say(format!("error: {e}")),
                }
            }
            Act::Quit => return false,
        }
        true
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
            KeyCode::Char('o') if ctrl => return self.act(Act::Home),
            KeyCode::Char('x') if ctrl => return self.act(Act::Shot),
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
        let (Some(placed), Some(screen)) = (self.placed, self.screen) else { return true };
        let at = placed.points_at(m.column, m.row, screen);
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) if m.row < TOP => {
                if let Some(a) = self.button_at(m.column) {
                    return self.act(a);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => self.press = at.is_some().then_some((m.column, m.row)),
            MouseEventKind::Up(MouseButton::Left) => {
                let Some((c0, r0)) = self.press.take() else { return true };
                let Some(from) = placed.points_at(c0, r0, screen) else { return true };
                let to = placed
                    .points_at(m.column.clamp(placed.left, placed.left + placed.cols - 1), m.row.clamp(TOP, TOP + placed.rows - 1), screen)
                    .unwrap_or(from);
                let udid = self.dev.udid.clone();
                if c0.abs_diff(m.column) + r0.abs_diff(m.row) <= 1 {
                    let _ = state::event(&udid, "user tap", &format!("{:.0},{:.0}", from.0, from.1));
                    self.input(move |u| axe::tap(u, from.0, from.1));
                } else {
                    let _ = state::event(&udid, "user swipe", &format!("{:.0},{:.0} -> {:.0},{:.0}", from.0, from.1, to.0, to.1));
                    self.input(move |u| axe::swipe(u, from, to, 0.3));
                }
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                if let Some((x, y)) = at {
                    let dy = if m.kind == MouseEventKind::ScrollDown { -120.0 } else { 120.0 };
                    let to = (x, (y + dy).clamp(1.0, screen.1 - 1.0));
                    self.input(move |u| axe::swipe(u, (x, y), to, 0.15));
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
        message: "click to tap, drag to swipe, type to type · Ctrl-O home, Ctrl-X screenshot, Ctrl-Q close".into(),
        message_at: Instant::now(),
        press: None,
        typed: String::new(),
    };
    let result = event_loop(&mut v, &rx);
    v.stop_stream();
    result
}

fn event_loop(v: &mut View, rx: &Receiver<Msg>) -> R<()> {
    let mut last_tick = Instant::now() - Duration::from_secs(10);
    loop {
        if last_tick.elapsed() >= Duration::from_secs(1) {
            last_tick = Instant::now();
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
            continue;
        }
        loop {
            let keep = match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => v.on_key(k),
                Event::Mouse(m) => v.on_mouse(m),
                Event::Resize(..) => {
                    v.placed = None;
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
    fn rescale_targets_the_drawn_width() {
        assert_eq!(rescale(0.5, 600, 300.0), 0.25);
        assert_eq!(rescale(0.5, 100, 1000.0), 1.0);
        assert_eq!(rescale(0.5, 6000, 10.0), 0.1);
    }
}
