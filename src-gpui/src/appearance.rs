use gpui::component::{Theme, ThemeMode};
use gpui::{App, Window};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preference {
    System,
    Light,
    Dark,
}
static PREFERENCE: AtomicU8 = AtomicU8::new(0);
static DARK: AtomicBool = AtomicBool::new(false);
impl Preference {
    fn parse(value: &str) -> Self {
        match value.trim() {
            "light" => Self::Light,
            "dark" => Self::Dark,
            _ => Self::System,
        }
    }
    fn code(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}
pub fn preference() -> Preference {
    match PREFERENCE.load(Ordering::Relaxed) {
        1 => Preference::Light,
        2 => Preference::Dark,
        _ => Preference::System,
    }
}
fn path() -> std::io::Result<std::path::PathBuf> {
    dirs::config_dir()
        .map(|p| p.join("com.s3studio.desktop").join("theme"))
        .ok_or_else(|| std::io::Error::other("Unable to locate settings directory"))
}
pub fn initialize() {
    let saved = path()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    PREFERENCE.store(Preference::parse(&saved) as u8, Ordering::Relaxed);
}
pub fn select(value: Preference, window: &mut Window, cx: &mut App) -> std::io::Result<()> {
    let path = path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, value.code())?;
    std::fs::rename(temporary, path)?;
    PREFERENCE.store(value as u8, Ordering::Relaxed);
    apply(Some(window), cx);
    Ok(())
}
pub fn apply(window: Option<&mut Window>, cx: &mut App) {
    // Theme::change only updates GPUI controls; macOS window chrome uses
    // NSApplication.appearance. Clear the override when following the system.
    cx.set_window_appearance(match preference() {
        Preference::System => None,
        Preference::Light => Some(gpui::WindowAppearance::Light),
        Preference::Dark => Some(gpui::WindowAppearance::Dark),
    });
    let mode = match preference() {
        Preference::Light => ThemeMode::Light,
        Preference::Dark => ThemeMode::Dark,
        Preference::System => window
            .as_ref()
            .map(|w| w.appearance())
            .unwrap_or_else(|| cx.window_appearance())
            .into(),
    };
    DARK.store(mode.is_dark(), Ordering::Relaxed);
    Theme::change(mode, window, cx);
    Theme::update(cx, |theme| {
        theme.primary = gpui::rgb(0x007aff).into();
        theme.primary_hover = gpui::rgb(0x006deb).into();
        theme.primary_active = gpui::rgb(0x006deb).into();
        if !mode.is_dark() {
            theme.foreground = gpui::rgb(0x4b5362).into();
            theme.muted_foreground = gpui::rgb(0xaeb5c2).into();
            theme.input = gpui::rgb(0xe0e5ec).into();
        }
        theme.primary_foreground = gpui::rgb(0xffffff).into();
        theme.ring = gpui::rgb(0x007aff).into();
    });
}
pub fn sync(window: &mut Window, cx: &mut App) {
    let mode: ThemeMode = window.appearance().into();
    if preference() == Preference::System && mode.is_dark() != DARK.load(Ordering::Relaxed) {
        apply(Some(window), cx);
    }
}
// The original interface uses a fixed light palette. Keep accent hues, while
// mapping its neutral text, borders and tinted surfaces to dark equivalents.
fn color(value: u32, dark: bool) -> u32 {
    if !dark {
        return value;
    }
    let r = ((value >> 16) & 255) as f32 / 255.;
    let g = ((value >> 8) & 255) as f32 / 255.;
    let b = (value & 255) as f32 / 255.;
    let min = r.min(g).min(b);
    let max = r.max(g).max(b);
    if min > 0.78 {
        let base = 0.12 + (1. - min) * 0.65;
        return pack(
            base + (r - min) * 0.25,
            base + (g - min) * 0.25,
            base + (b - min) * 0.25,
        );
    }
    if max - min < 0.18 {
        let lift = (0.90 - (r + g + b) / 3. * 0.35) - (r + g + b) / 3.;
        return pack(r + lift, g + lift, b + lift);
    }
    value
}
fn pack(r: f32, g: f32, b: f32) -> u32 {
    let channel = |v: f32| (v.clamp(0., 1.) * 255.).round() as u32;
    channel(r) << 16 | channel(g) << 8 | channel(b)
}
pub fn rgb(value: u32) -> gpui::Rgba {
    gpui::rgb(color(value, DARK.load(Ordering::Relaxed)))
}
pub fn rgba(value: u32) -> gpui::Rgba {
    if value == 0x1c29433d {
        return gpui::rgba(value);
    }
    gpui::rgba((color(value >> 8, DARK.load(Ordering::Relaxed)) << 8) | (value & 255))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preferences_and_palette() {
        for preference in [Preference::System, Preference::Light, Preference::Dark] {
            assert_eq!(Preference::parse(preference.code()), preference);
        }
        assert_eq!(Preference::parse("invalid"), Preference::System);
        assert_eq!(color(0xffffff, false), 0xffffff);
        assert!(color(0xffffff, true) < 0x404040);
        assert!(color(0x252731, true) > 0xaaaaaa);
        assert_eq!(color(0x007aff, true), 0x007aff);
    }
}
