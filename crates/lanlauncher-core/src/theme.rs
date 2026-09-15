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

impl Theme {
    pub fn parse(json: &str) -> crate::Result<Self> {
        let theme: Theme = serde_json::from_str(json)?;
        theme.validate()?;
        Ok(theme)
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
