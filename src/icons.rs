//! View header icons in herdr desktop's style (24-unit stroke icons, round caps), rasterized
//! for Kitty graphics; glyphs stand in where images aren't available.
//! Copied from herdr-web-browser with this plugin's icon set.

use crate::R;
use resvg::{tiny_skia, usvg};

pub type Rgb = (u8, u8, u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Home,
    Lock,
    Camera,
    Alert,
    Close,
}

pub const ALL: [Icon; 5] = [Icon::Home, Icon::Lock, Icon::Camera, Icon::Alert, Icon::Close];

impl Icon {
    fn body(self) -> &'static str {
        match self {
            Icon::Home => {
                r#"<path d="M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8"/><path d="M3 10a2 2 0 0 1 .71-1.53l7-6a2 2 0 0 1 2.58 0l7 6A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>"#
            }
            Icon::Lock => r#"<rect x="4" y="11" width="16" height="10" rx="2"/><path d="M8 11V7a4 4 0 0 1 8 0v4"/>"#,
            Icon::Camera => r#"<path d="M4 8.5h3l1.8-2.5h6.4L17 8.5h3V19H4z"/><circle cx="12" cy="13.5" r="3.2"/>"#,
            Icon::Alert => {
                r#"<path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3"/><path d="M12 9v4"/><path d="M12 17h.01"/>"#
            }
            Icon::Close => r#"<path d="M18 6 6 18"/><path d="m6 6 12 12"/>"#,
        }
    }

    /// Fallback when the terminal can't show images.
    pub fn glyph(self) -> &'static str {
        match self {
            Icon::Home => "⌂",
            Icon::Lock => "⏻",
            Icon::Camera => "◉",
            Icon::Alert => "!",
            Icon::Close => "✕",
        }
    }

    /// PNG of the icon centered in a `w`×`h` px transparent box, filling `scale` of its height.
    pub fn png(self, color: Rgb, w: u32, h: u32, scale: f32) -> R<Vec<u8>> {
        let side = h as f32 * scale;
        let (dx, dy) = ((w as f32 - side) / 2.0, (h as f32 - side) / 2.0);
        render(&format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}">{}</svg>"#, self.group(color, dx, dy, side)), w, h)
    }

    fn group(self, color: Rgb, dx: f32, dy: f32, side: f32) -> String {
        format!(
            r#"<g transform="translate({dx} {dy}) scale({k})" fill="none" stroke="{c}" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">{body}</g>"#,
            k = side / 24.0,
            c = hex(color),
            body = self.body()
        )
    }
}

fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

fn render(svg: &str, w: u32, h: u32) -> R<Vec<u8>> {
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default())?;
    let mut pixmap = tiny_skia::Pixmap::new(w.max(1), h.max(1)).ok_or("bad image size")?;
    resvg::render(&tree, tiny_skia::Transform::identity(), &mut pixmap.as_mut());
    Ok(pixmap.encode_png()?)
}

/// A hovered button: the icon on a rounded pill, like herdr desktop's hover state.
pub fn pill_png(icon: Icon, fg: Rgb, bg: Rgb, w: u32, h: u32) -> R<Vec<u8>> {
    let (inset, side) = (h as f32 * 0.1, h as f32 * 0.62);
    let r = (h as f32 - 2.0 * inset) * 0.32;
    let (dx, dy) = ((w as f32 - side) / 2.0, (h as f32 - side) / 2.0);
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}"><rect x="{inset}" y="{inset}" width="{rw}" height="{rh}" rx="{r}" fill="{bg}"/>{g}</svg>"#,
        rw = w as f32 - 2.0 * inset,
        rh = h as f32 - 2.0 * inset,
        bg = hex(bg),
        g = icon.group(fg, dx, dy, side)
    );
    render(&svg, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_rasterizes_with_visible_pixels() {
        for icon in ALL {
            for png in [icon.png((255, 255, 255), 34, 44, 0.62).unwrap(), pill_png(icon, (255, 255, 255), (0, 0, 0), 68, 44).unwrap()] {
                let px = tiny_skia::Pixmap::decode_png(&png).unwrap();
                assert!(px.pixels().iter().any(|p| p.alpha() > 0), "{icon:?} is blank");
            }
        }
    }
}
