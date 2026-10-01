//! Colours from the current Omarchy theme
//! (`~/.local/state/omarchy/current/theme/colors.toml`), plus a cheap
//! watcher so frontends re-skin live after `omarchy theme set`.

use std::path::PathBuf;

use crate::store::Stamp;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn parse(hex: &str) -> Option<Rgb> {
        let h = hex.trim().trim_start_matches('#');
        // Accept #rrggbb and #rrggbbaa (alpha ignored).
        if h.len() != 6 && h.len() != 8 {
            return None;
        }
        let v = u32::from_str_radix(&h[..6], 16).ok()?;
        Some(Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
    }

    /// Linear blend towards `other` (t = 0 → self, 1 → other).
    pub fn mix(self, other: Rgb, t: f32) -> Rgb {
        let f = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Rgb(f(self.0, other.0), f(self.1, other.1), f(self.2, other.2))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub dark: bool,
    /// Main window background.
    pub bg: Rgb,
    /// Sidebars and chrome, a step darker than `bg`.
    pub panel: Rgb,
    /// Desktop behind floating things, darkest.
    pub deep: Rgb,
    /// Hover / raised surfaces.
    pub raised: Rgb,
    pub selection: Rgb,
    pub fg: Rgb,
    pub dim: Rgb,
    pub muted: Rgb,
    pub accent: Rgb,
    pub green: Rgb,
    pub red: Rgb,
    pub yellow: Rgb,
    pub cyan: Rgb,
}

impl Default for Theme {
    /// Catppuccin Mocha, used when no Omarchy theme is found.
    fn default() -> Theme {
        Theme::from_toml("")
    }
}

impl Theme {
    pub fn from_toml(text: &str) -> Theme {
        let mut kv: Vec<(&str, &str)> = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                let v = v.trim();
                let v = v.split(" #").next().unwrap_or(v); // trailing comment
                kv.push((k.trim(), v.trim().trim_matches('"').trim_matches('\'')));
            }
        }
        let get = |k: &str| kv.iter().find(|(key, _)| *key == k).map(|(_, v)| *v);
        let pick = |keys: &[&str], fb: &str| -> Rgb {
            keys.iter()
                .find_map(|k| get(k).and_then(Rgb::parse))
                .unwrap_or_else(|| Rgb::parse(fb).unwrap())
        };

        let bg = pick(&["background"], "#1e1e2e");
        let fg = pick(&["foreground"], "#cdd6f4");
        let dark = match get("mode") {
            Some(m) => !m.eq_ignore_ascii_case("light"),
            None => luminance(bg) < luminance(fg),
        };
        // Older themes lack the darker/lighter variants; derive them.
        let toward_deep = if dark { Rgb(0, 0, 0) } else { Rgb(255, 255, 255) };
        let toward_fg = fg;
        Theme {
            dark,
            bg,
            panel: get("dark_background").and_then(Rgb::parse).unwrap_or(bg.mix(toward_deep, 0.18)),
            deep: get("darker_background").and_then(Rgb::parse).unwrap_or(bg.mix(toward_deep, 0.35)),
            raised: get("lighter_background").and_then(Rgb::parse).unwrap_or(bg.mix(toward_fg, 0.1)),
            selection: get("selection").and_then(Rgb::parse).unwrap_or(bg.mix(toward_fg, 0.2)),
            fg,
            dim: pick(&["dark_foreground", "muted"], "#6c7086"),
            muted: pick(&["muted", "dark_foreground"], "#585b70"),
            accent: pick(&["accent", "blue"], "#89b4fa"),
            green: pick(&["green", "bright_green"], "#a6e3a1"),
            red: pick(&["red", "bright_red"], "#f38ba8"),
            yellow: pick(&["yellow", "bright_yellow"], "#f9e2af"),
            cyan: pick(&["cyan", "bright_cyan"], "#94e2d5"),
        }
    }

    pub fn load() -> Theme {
        theme_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|t| Theme::from_toml(&t))
            .unwrap_or_default()
    }
}

fn luminance(c: Rgb) -> f32 {
    0.2126 * c.0 as f32 + 0.7152 * c.1 as f32 + 0.0722 * c.2 as f32
}

pub fn theme_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".local/state/omarchy/current/theme/colors.toml"))
}

/// Polled from the UI loop (about twice a second). `omarchy theme set`
/// swaps the whole theme directory, which changes the file's inode, so a
/// stat is enough to notice.
pub struct ThemeWatch {
    path: Option<PathBuf>,
    stamp: Option<Stamp>,
    current: Theme,
}

impl ThemeWatch {
    pub fn new() -> ThemeWatch {
        let path = theme_path();
        let stamp = path.as_deref().and_then(Stamp::of);
        ThemeWatch { path, stamp, current: Theme::load() }
    }

    pub fn theme(&self) -> Theme {
        self.current
    }

    /// Returns the new theme if the file changed since the last call.
    pub fn poll(&mut self) -> Option<Theme> {
        let path = self.path.as_deref()?;
        let stamp = Stamp::of(path);
        // Mid-swap the file can be briefly missing; keep the old colours.
        if stamp.is_none() || stamp == self.stamp {
            return None;
        }
        self.stamp = stamp;
        let theme = Theme::from_toml(&std::fs::read_to_string(path).ok()?);
        if theme == self.current {
            return None;
        }
        self.current = theme;
        Some(theme)
    }
}

impl Default for ThemeWatch {
    fn default() -> Self {
        ThemeWatch::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_omarchy_colors_toml() {
        let t = Theme::from_toml(
            "mode = \"light\"\naccent = \"#1e66f5\"\nbackground = \"#eff1f5\"\nforeground = '#4c4f69' # ink\n[extra]\nred = \"#d20f39ff\"\n",
        );
        assert!(!t.dark);
        assert_eq!(t.accent, Rgb(0x1e, 0x66, 0xf5));
        assert_eq!(t.fg, Rgb(0x4c, 0x4f, 0x69));
        assert_eq!(t.red, Rgb(0xd2, 0x0f, 0x39));
        // derived from background when the theme lacks them
        assert_ne!(t.panel, t.bg);
    }

    #[test]
    fn guesses_mode_without_key() {
        assert!(Theme::from_toml("background = \"#000000\"\nforeground = \"#ffffff\"").dark);
        assert!(!Theme::from_toml("background = \"#ffffff\"\nforeground = \"#000000\"").dark);
    }

    #[test]
    fn rejects_bad_hex() {
        assert!(Rgb::parse("#12345").is_none());
        assert!(Rgb::parse("zzzzzz").is_none());
        assert_eq!(Rgb::parse("ffffff"), Some(Rgb(255, 255, 255)));
    }
}
