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
            accent: "#ff9f43".into(),
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
            accent: "#d9771c".into(),
            success: "#1f9d5a".into(),
            warning: "#c99a06".into(),
            danger: "#d94848".into(),
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
                } else {
                    log::warn!("launcher.ini: {key} is not a colour: {value}");
                }
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

#[cfg(test)]
mod tests {

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
