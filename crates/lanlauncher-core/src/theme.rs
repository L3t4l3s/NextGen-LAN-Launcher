//! Visual theme of the launcher.
//!
//! A theme is a small JSON document an organiser can ship with the event
//! (`http://launcher.lan/theme.json`) or a user can pick locally. Only colours,
//! images and a few shape tokens are themable; layout stays fixed so every LAN
//! launcher remains recognisable and support instructions stay valid.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ThemeColors {
    pub background: String,
    pub surface: String,
    pub surface_alt: String,
    pub text: String,
    pub text_muted: String,
    pub primary: String,
    pub primary_text: String,
    pub accent: String,
    pub success: String,
    pub warning: String,
    pub danger: String,
    pub border: String,
}

impl Default for ThemeColors {
    fn default() -> Self {
        Self {
            background: "#0f1218".into(),
            surface: "#171c25".into(),
            surface_alt: "#1f2632".into(),
            text: "#f2f4f8".into(),
            text_muted: "#9aa4b5".into(),
            primary: "#4f8cff".into(),
            primary_text: "#ffffff".into(),
            accent: "#7ee8ff".into(),
            success: "#2ecc71".into(),
            warning: "#f1c40f".into(),
            danger: "#ff5c5c".into(),
            border: "#2a3140".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Theme {
    /// Schema version for forward compatibility.
    pub version: u32,
    pub name: String,
    /// Dark or light base; affects default contrast choices in the UI.
    pub mode: ThemeMode,
    pub colors: ThemeColors,
    /// Logo shown in the top bar (URL, data URI or path relative to the theme).
    pub logo: Option<String>,
    /// Optional background image behind the content.
    pub background_image: Option<String>,
    /// Corner radius token in pixels.
    pub radius: u32,
    /// Font stack override.
    pub font_family: Option<String>,
    /// Override icons by name (`library`, `downloads`, `lan`, ...) with SVG/URL.
    pub icons: BTreeMap<String, String>,
    /// Legacy ETI `launcher.css` to inject as an extra stylesheet, if any.
    pub legacy_css: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            version: 1,
            name: "NextGen Dark".into(),
            mode: ThemeMode::Dark,
            colors: ThemeColors::default(),
            logo: None,
            background_image: None,
            radius: 12,
            font_family: None,
            icons: BTreeMap::new(),
            legacy_css: None,
        }
    }
}

impl ThemeColors {
    /// The light base, mirroring `themes/light.json`. Overriding a single
    /// colour on the dark base would otherwise leave white text on a white
    /// background for a LANPage that only says `theme_mode = light`.
    pub fn light() -> Self {
        Self {
            background: "#f3f5f9".into(),
            surface: "#ffffff".into(),
            surface_alt: "#e9edf4".into(),
            text: "#161b26".into(),
            text_muted: "#5c6779".into(),
            primary: "#2f6fed".into(),
            primary_text: "#ffffff".into(),
            accent: "#c026d3".into(),
            success: "#15803d".into(),
            warning: "#854d0e".into(),
            danger: "#dc2626".into(),
            border: "#d3d9e4".into(),
        }
    }
}

impl Theme {
    pub fn parse(json: &str) -> crate::Result<Self> {
        let theme: Theme = serde_json::from_str(json)?;
        theme.validate()?;
        Ok(theme)
    }

    /// A theme from the `theme_…` keys of the LANPage's `launcher.ini`.
    ///
    /// `theme.json` is the full surface; this is the small door for a LANPage
    /// that only wants its own colours and would rather add three lines to a
    /// file it already serves than host another one. Unknown and malformed
    /// values are ignored rather than rejected: a typo in one colour must not
    /// cost the organiser the rest of the theme. `None` when the ini names
    /// nothing themable at all.
    ///
    /// Every value ends up in a CSS custom property, so each one passes
    /// [`is_safe_css_color`] first.
    pub fn from_ini(extra: &BTreeMap<String, String>) -> Option<Self> {
        let mut theme = Theme::default();
        let mut any = false;
        if let Some(mode) = extra
            .get("theme_mode")
            .map(|m| m.trim().to_ascii_lowercase())
        {
            match mode.as_str() {
                "light" => {
                    theme.mode = ThemeMode::Light;
                    theme.colors = ThemeColors::light();
                    // The name reaches the log; "NextGen Dark" next to a
                    // light palette is what a reader has to unlearn again.
                    theme.name = "NextGen Light".into();
                    any = true;
                }
                "dark" => any = true,
                other => log::warn!("launcher.ini: theme_mode is neither dark nor light: {other}"),
            }
        }
        let mut primary_set = false;
        let mut primary_text_set = false;
        {
            let c = &mut theme.colors;
            for (key, slot) in [
                ("theme_background", &mut c.background),
                ("theme_surface", &mut c.surface),
                ("theme_surface_alt", &mut c.surface_alt),
                ("theme_text", &mut c.text),
                ("theme_text_muted", &mut c.text_muted),
                ("theme_primary", &mut c.primary),
                ("theme_primary_text", &mut c.primary_text),
                ("theme_accent", &mut c.accent),
                ("theme_success", &mut c.success),
                ("theme_warning", &mut c.warning),
                ("theme_danger", &mut c.danger),
                ("theme_border", &mut c.border),
            ] {
                let Some(value) = extra.get(key).map(|v| v.trim()) else {
                    continue;
                };
                if is_safe_css_color(value) {
                    *slot = value.to_string();
                    any = true;
                    primary_set |= key == "theme_primary";
                    primary_text_set |= key == "theme_primary_text";
                } else {
                    log::warn!("launcher.ini: {key} is not a colour: {value}");
                }
            }
        }
        // A LANPage that names only `theme_primary` would keep the built-in
        // white button text; on a light primary that is unreadable. Deriving
        // it is only right where the organiser said nothing about it.
        if primary_set && !primary_text_set {
            if let Some(ink) = readable_on(&theme.colors.primary) {
                theme.colors.primary_text = ink.to_string();
            }
        }
        if let Some(name) = extra
            .get("theme_name")
            .map(|n| n.trim())
            .filter(|n| !n.is_empty() && n.chars().count() <= 64)
        {
            theme.name = name.to_string();
        }
        if let Some(raw) = extra.get("theme_radius").map(|r| r.trim()) {
            match raw.parse::<u32>() {
                Ok(radius) if radius <= 48 => {
                    theme.radius = radius;
                    any = true;
                }
                _ => log::warn!("launcher.ini: theme_radius is not a number up to 48: {raw}"),
            }
        }
        any.then_some(theme)
    }

    /// Reject values that could break the UI or inject markup.
    pub fn validate(&self) -> crate::Result<()> {
        let all = [
            &self.colors.background,
            &self.colors.surface,
            &self.colors.surface_alt,
            &self.colors.text,
            &self.colors.text_muted,
            &self.colors.primary,
            &self.colors.primary_text,
            &self.colors.accent,
            &self.colors.success,
            &self.colors.warning,
            &self.colors.danger,
            &self.colors.border,
        ];
        for c in all {
            if !is_safe_css_color(c) {
                return Err(crate::Error::Settings(format!(
                    "invalid colour `{c}` in theme"
                )));
            }
        }
        if self.radius > 48 {
            return Err(crate::Error::Settings("radius must be <= 48".into()));
        }
        Ok(())
    }

    /// Render the theme as CSS custom properties for the `:root` element.
    pub fn to_css_variables(&self) -> String {
        let c = &self.colors;
        let mut css = String::new();
        let mut push = |k: &str, v: &str| css.push_str(&format!("--{k}: {v};\n"));
        push("color-bg", &c.background);
        push("color-surface", &c.surface);
        push("color-surface-alt", &c.surface_alt);
        push("color-text", &c.text);
        push("color-text-muted", &c.text_muted);
        push("color-primary", &c.primary);
        push("color-primary-text", &c.primary_text);
        push("color-accent", &c.accent);
        push("color-success", &c.success);
        push("color-warning", &c.warning);
        push("color-danger", &c.danger);
        push("color-border", &c.border);
        push("radius", &format!("{}px", self.radius));
        if let Some(f) = &self.font_family {
            let cleaned: String = f
                .chars()
                .filter(|ch| !matches!(ch, ';' | '{' | '}' | '<' | '>'))
                .collect();
            push("font-family", &cleaned);
        }
        css
    }
}

/// Accept hex colours, `rgb()/rgba()/hsl()/hsla()` and plain CSS colour names.
pub fn is_safe_css_color(value: &str) -> bool {
    let v = value.trim();
    if v.is_empty() || v.len() > 64 {
        return false;
    }
    if let Some(hex) = v.strip_prefix('#') {
        return matches!(hex.len(), 3 | 4 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit());
    }
    if v.chars().all(|c| c.is_ascii_alphabetic()) {
        return true;
    }
    let lower = v.to_ascii_lowercase();
    (lower.starts_with("rgb(")
        || lower.starts_with("rgba(")
        || lower.starts_with("hsl(")
        || lower.starts_with("hsla("))
        && lower.ends_with(')')
        && lower.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '(' | ')' | ',' | '.' | '%' | ' ' | '/' | '-')
        })
}

/// Black or white, whichever reads better on `background`, or `None` when the
/// colour cannot be read at all — a caller must then keep what it had rather
/// than guess.
pub fn readable_on(background: &str) -> Option<&'static str> {
    const DARK: &str = "#0d1318";
    const LIGHT: &str = "#ffffff";
    let rgb = parse_color(background)?;
    let channel = |v: f64| {
        let c = v / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let luminance = 0.2126 * channel(rgb[0]) + 0.7152 * channel(rgb[1]) + 0.0722 * channel(rgb[2]);
    let ratio = |ink: f64| {
        let (a, b) = if luminance > ink {
            (luminance, ink)
        } else {
            (ink, luminance)
        };
        (a + 0.05) / (b + 0.05)
    };
    // The two inks as luminance: #0d1318 is near black, #ffffff is white.
    Some(if ratio(1.0) > ratio(0.006) {
        LIGHT
    } else {
        DARK
    })
}

/// `#rgb`, `#rrggbb(aa)`, `rgb()/rgba()` and `hsl()/hsla()` as 0-255 triples.
/// Named colours are not resolved; there are 148 of them and the only caller
/// can do without.
fn parse_color(value: &str) -> Option<[f64; 3]> {
    let v = value.trim();
    if let Some(hex) = v.strip_prefix('#') {
        let bytes: Vec<f64> = match hex.len() {
            3 | 4 => hex
                .chars()
                .take(3)
                .filter_map(|c| u8::from_str_radix(&format!("{c}{c}"), 16).ok())
                .map(f64::from)
                .collect(),
            6 | 8 => (0..3)
                .filter_map(|i| u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok())
                .map(f64::from)
                .collect(),
            _ => return None,
        };
        return (bytes.len() == 3).then(|| [bytes[0], bytes[1], bytes[2]]);
    }
    let lower = v.to_ascii_lowercase();
    let (kind, rest) = lower.split_once('(')?;
    let inner = rest.strip_suffix(')')?;
    let parts: Vec<&str> = inner
        .split([',', ' ', '/'])
        .filter(|p| !p.is_empty())
        .collect();
    let number = |s: &str| -> Option<f64> { s.trim_end_matches('%').parse::<f64>().ok() };
    match kind.trim() {
        "rgb" | "rgba" => {
            let scale = |s: &str| -> Option<f64> {
                let n = number(s)?;
                Some(if s.ends_with('%') {
                    n * 255.0 / 100.0
                } else {
                    n
                })
            };
            Some([
                scale(parts.first()?)?,
                scale(parts.get(1)?)?,
                scale(parts.get(2)?)?,
            ])
        }
        "hsl" | "hsla" => {
            let h = number(parts.first()?)? / 360.0;
            let s = number(parts.get(1)?)? / 100.0;
            let l = number(parts.get(2)?)? / 100.0;
            let hue = |mut t: f64| -> f64 {
                if t < 0.0 {
                    t += 1.0;
                }
                if t > 1.0 {
                    t -= 1.0;
                }
                let q = if l < 0.5 {
                    l * (1.0 + s)
                } else {
                    l + s - l * s
                };
                let p = 2.0 * l - q;
                let c = if t < 1.0 / 6.0 {
                    p + (q - p) * 6.0 * t
                } else if t < 0.5 {
                    q
                } else if t < 2.0 / 3.0 {
                    p + (q - p) * (2.0 / 3.0 - t) * 6.0
                } else {
                    p
                };
                c * 255.0
            };
            Some([hue(h + 1.0 / 3.0), hue(h), hue(h - 1.0 / 3.0)])
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn the_dark_base_matches_the_shipped_default_theme() {
        let shipped = Theme::parse(include_str!("../../../themes/default.json")).unwrap();
        assert_eq!(shipped.colors, ThemeColors::default());
    }

    #[test]
    fn the_light_base_matches_the_shipped_light_theme() {
        // `ThemeColors::light()` is what `theme_mode = light` starts from; it
        // must not drift away from the theme users can pick in the settings.
        let shipped = Theme::parse(include_str!("../../../themes/light.json")).unwrap();
        assert_eq!(shipped.colors, ThemeColors::light());
    }

    fn ini(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn colours_from_launcher_ini_keep_the_rest_of_the_defaults() {
        let theme = Theme::from_ini(&ini(&[
            ("theme_primary", "#29b6f6"),
            ("theme_background", "#0b1a2b"),
            ("theme_name", "Next Generation LAN"),
            ("lan_title", "irrelevant"),
        ]))
        .expect("a theme");
        assert_eq!(theme.colors.primary, "#29b6f6");
        assert_eq!(theme.colors.background, "#0b1a2b");
        assert_eq!(theme.name, "Next Generation LAN");
        // Untouched keys keep the built-in dark values.
        assert_eq!(theme.colors.surface, ThemeColors::default().surface);
        theme.validate().expect("valid");
    }

    #[test]
    fn a_light_lanpage_gets_the_light_base() {
        let theme = Theme::from_ini(&ini(&[
            ("theme_mode", "Light"),
            ("theme_accent", "#d9771c"),
        ]))
        .unwrap();
        assert_eq!(theme.mode, ThemeMode::Light);
        // Not the dark background with light text on top of it.
        assert_eq!(theme.colors.background, ThemeColors::light().background);
        assert_eq!(theme.colors.text, ThemeColors::light().text);
        assert_eq!(theme.colors.accent, "#d9771c");
        assert_eq!(theme.name, "NextGen Light");
        // A name in the ini still wins over it.
        let named = Theme::from_ini(&ini(&[
            ("theme_mode", "light"),
            ("theme_name", "Sommer-LAN"),
        ]))
        .unwrap();
        assert_eq!(named.name, "Sommer-LAN");
    }

    #[test]
    fn a_light_primary_gets_dark_button_text() {
        // White on #ffd166 is about 1.4:1 — the label on every primary button.
        let theme = Theme::from_ini(&ini(&[("theme_primary", "#ffd166")])).unwrap();
        assert_eq!(theme.colors.primary_text, "#0d1318");
        // A dark primary keeps the light ink.
        let dark = Theme::from_ini(&ini(&[("theme_primary", "#1d4ed8")])).unwrap();
        assert_eq!(dark.colors.primary_text, "#ffffff");
        // Notations other than hex are understood too.
        let rgb = Theme::from_ini(&ini(&[("theme_primary", "rgb(255, 209, 102)")])).unwrap();
        assert_eq!(rgb.colors.primary_text, "#0d1318");
        let hsl = Theme::from_ini(&ini(&[("theme_primary", "hsl(220, 70%, 30%)")])).unwrap();
        assert_eq!(hsl.colors.primary_text, "#ffffff");
        // A colour nobody can read (a name) leaves the built-in ink alone
        // instead of guessing.
        let named = Theme::from_ini(&ini(&[("theme_primary", "gold")])).unwrap();
        assert_eq!(
            named.colors.primary_text,
            ThemeColors::default().primary_text
        );
        // A typo in the key name must not trigger the derivation either.
        let typo = Theme::from_ini(&ini(&[
            ("theme_primry", "#ffd166"),
            ("theme_accent", "#ffd166"),
        ]))
        .unwrap();
        assert_eq!(
            typo.colors.primary_text,
            ThemeColors::default().primary_text
        );

        // What the organiser said themselves is never second-guessed.
        let explicit = Theme::from_ini(&ini(&[
            ("theme_primary", "#ffd166"),
            ("theme_primary_text", "#333333"),
        ]))
        .unwrap();
        assert_eq!(explicit.colors.primary_text, "#333333");
    }

    #[test]
    fn a_broken_colour_costs_only_itself() {
        // Everything here ends up in a CSS custom property, so a value that
        // could close the declaration must never reach it.
        let theme = Theme::from_ini(&ini(&[
            ("theme_primary", "#29b6f6"),
            ("theme_accent", "red; } body { display: none"),
        ]))
        .unwrap();
        assert_eq!(theme.colors.primary, "#29b6f6");
        assert_eq!(theme.colors.accent, ThemeColors::default().accent);
        theme.validate().expect("valid");
    }

    #[test]
    fn an_ini_without_theme_keys_is_no_theme() {
        assert!(Theme::from_ini(&ini(&[("lan_title", "LAN")])).is_none());
        // A name alone changes nothing visible and must not override a
        // built-in theme the user picked.
        assert!(Theme::from_ini(&ini(&[("theme_name", "LAN")])).is_none());
    }
    use super::*;

    #[test]
    fn default_theme_is_valid_and_renders_css() {
        let t = Theme::default();
        t.validate().unwrap();
        let css = t.to_css_variables();
        assert!(css.contains("--color-primary: #4f8cff;"));
        assert!(css.contains("--radius: 12px;"));
    }

    #[test]
    fn rejects_css_injection_in_colors() {
        let json = r##"{"colors":{"primary":"red; background:url(evil)"}}"##;
        assert!(Theme::parse(json).is_err());
        let json = r##"{"colors":{"primary":"rgba(10, 20, 30, 0.5)"}, "name":"x"}"##;
        assert!(Theme::parse(json).is_ok());
    }

    #[test]
    fn partial_theme_merges_with_defaults() {
        let t = Theme::parse(r##"{"name":"Pink LAN","colors":{"primary":"#ff00aa"},"radius":4}"##)
            .unwrap();
        assert_eq!(t.name, "Pink LAN");
        assert_eq!(t.colors.primary, "#ff00aa");
        assert_eq!(t.colors.background, ThemeColors::default().background);
        assert_eq!(t.radius, 4);
    }
}
