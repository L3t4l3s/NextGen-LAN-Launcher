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
    /// Top bar; `None` keeps `surface`. A LANPage usually has a band of its
    /// own colour up there, and matching it is most of "looks like the page".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    /// Text and icons on `header`; `None` keeps `text`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header_text: Option<String>,
    /// Status bar; `None` keeps `surface`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footer: Option<String>,
    /// Text on `footer`; `None` keeps `text_muted`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footer_text: Option<String>,
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
            header: None,
            header_text: None,
            footer: None,
            footer_text: None,
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
    /// Logo shown in the top bar: an `http(s)` URL or a `data:image/…` URI.
    /// A relative path is dropped — the theme is applied inside the launcher,
    /// which has no idea where the file would sit.
    pub logo: Option<String>,
    /// Background image behind the content, same rule as `logo`.
    pub background_image: Option<String>,
    /// Colour laid over `background_image` so text stays readable on a photo
    /// (`rgba(0,0,0,0.55)`). Ignored without an image.
    pub background_overlay: Option<String>,
    /// Corner radius token in pixels.
    pub radius: u32,
    /// Texture on the cards, tiles and the detail panel. A LANPage that draws
    /// its panels with scanlines or a grid cannot say so in a colour, and a
    /// stylesheet is not on offer; this names the figure and the launcher
    /// draws it. `None` leaves the surfaces flat.
    pub surface_pattern: Option<SurfacePattern>,
    /// Font stack override.
    pub font_family: Option<String>,
    /// Font stack for the headings (`h1`-`h3` and the event title in the top
    /// bar); `None` keeps `font_family`. A LANPage usually sets a display face
    /// on its headlines that would be unreadable as the body font, and naming
    /// only one stack meant the launcher either missed that face or wore it
    /// everywhere.
    pub heading_font_family: Option<String>,
    /// Font files the event brings along, named one by one. The launcher
    /// writes the `@font-face` rules itself from these; a LANPage never hands
    /// the launcher a stylesheet, so it can bring a font but not a free hand
    /// at the layout.
    pub font_faces: Vec<FontFace>,
    /// Override icons by name (`library`, `downloads`, `lan`, ...) with SVG/URL.
    pub icons: BTreeMap<String, String>,
    /// Legacy ETI `launcher.css` to inject as an extra stylesheet, if any.
    pub legacy_css: Option<String>,
}

/// The figure drawn on a surface. A closed list, because every kind becomes a
/// `background-image` the launcher writes itself — an organiser names the
/// figure and its two or three numbers, never the CSS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SurfacePatternKind {
    /// Flat, as without the key at all.
    #[default]
    None,
    /// Horizontal hairlines every `size` pixels — the CRT look.
    Scanlines,
    /// Scanlines in both directions.
    Grid,
    /// A dot every `size` pixels.
    Dots,
    /// Hairlines at `angle`.
    Diagonal,
    /// A wash from `color` at `angle` into nothing, over the surface colour.
    Gradient,
}

/// A texture on the surfaces, as far as a theme may describe one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SurfacePattern {
    /// The figure. An unknown name leaves the surfaces flat instead of
    /// costing the theme: a LANPage may well name a kind a newer launcher
    /// knows and this one does not.
    #[serde(deserialize_with = "kind_or_flat")]
    pub kind: SurfacePatternKind,
    /// The ink of the figure. Barely-there is the point: a scanline is a hint
    /// of a line, so this is usually an `rgba()` well under 0.1.
    #[serde(deserialize_with = "color_or_default")]
    pub color: String,
    /// Distance between the lines or dots in pixels, inside
    /// [`PATTERN_SIZE_RANGE`]; a value outside it is clamped.
    #[serde(deserialize_with = "size_or_default")]
    pub size: u32,
    /// Direction of `diagonal` and `gradient` in degrees; ignored by the rest.
    /// Anything is a direction once it is taken modulo a full turn.
    #[serde(deserialize_with = "angle_or_default")]
    pub angle: u32,
}

/// What a pattern looks like when a theme names the kind and nothing else.
/// A grey that shows on a dark card and on a light one, and the 4 px of the
/// CRT look.
pub const PATTERN_COLOR: &str = "rgba(128, 128, 128, 0.07)";
pub const PATTERN_SIZE: u32 = 4;
pub const PATTERN_ANGLE: u32 = 45;
/// Under 2 px a line pattern is a solid block, over 64 it is one line per
/// card. Both ends are the point where the figure stops being one.
pub const PATTERN_SIZE_RANGE: std::ops::RangeInclusive<u32> = 2..=64;

impl Default for SurfacePattern {
    fn default() -> Self {
        Self {
            kind: SurfacePatternKind::None,
            color: PATTERN_COLOR.into(),
            size: PATTERN_SIZE,
            angle: PATTERN_ANGLE,
        }
    }
}

/// What a file people edit by hand holds where a word belongs.
#[derive(Deserialize)]
#[serde(untagged)]
enum LooseText {
    Text(String),
    /// A number, `null`, a list — anything that is not a word.
    Other(serde::de::IgnoredAny),
}

/// The same where a number belongs. `"4"`, `4.0` and `-45` all turn up in a
/// theme somebody wrote by hand, and none of them is worth the rest of it.
#[derive(Deserialize)]
#[serde(untagged)]
enum LooseNumber {
    Int(i64),
    Float(f64),
    Text(String),
    Other(serde::de::IgnoredAny),
}

impl LooseNumber {
    /// The number behind the value, whatever notation it arrived in.
    fn value(&self) -> Option<i64> {
        let round = |f: f64| f.is_finite().then(|| f.round() as i64);
        match self {
            Self::Int(i) => Some(*i),
            Self::Float(f) => round(*f),
            Self::Text(t) => t.trim().parse::<f64>().ok().and_then(round),
            Self::Other(_) => None,
        }
    }
}

/// Distance between the lines, brought into the range where the figure is one.
/// A number outside it is clamped rather than refused: the same file has to
/// pass here and in `surfacePatternCss` of `src/lib/theme.ts`, which clamps,
/// and a theme that draws on one screen and is rejected on the next is worse
/// than a pattern two pixels off what was asked for.
fn size_or_default<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let (low, high) = (*PATTERN_SIZE_RANGE.start(), *PATTERN_SIZE_RANGE.end());
    Ok(match LooseNumber::deserialize(deserializer)?.value() {
        Some(v) => {
            let size = v.clamp(i64::from(low), i64::from(high)) as u32;
            if i64::from(size) != v {
                log::warn!("theme: surfacePattern.size {v} is outside {low}-{high}, using {size}");
            }
            size
        }
        None => {
            log::warn!("theme: surfacePattern.size is not a number, using {PATTERN_SIZE}");
            PATTERN_SIZE
        }
    })
}

/// The direction, as a turn: -45 is 315, 405 is 45.
fn angle_or_default<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(match LooseNumber::deserialize(deserializer)?.value() {
        Some(v) => v.rem_euclid(360) as u32,
        None => {
            log::warn!("theme: surfacePattern.angle is not a number, using {PATTERN_ANGLE}");
            PATTERN_ANGLE
        }
    })
}

/// The ink, as far as the type goes. A `null` or a number is the same slip as
/// the ones `kind`, `size` and `angle` forgive, so it costs the default rather
/// than the theme; a colour that *is* a word still has to be one the launcher
/// may paint with, which [`Theme::validate`] decides.
fn color_or_default<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let LooseText::Text(raw) = LooseText::deserialize(deserializer)? else {
        log::warn!("theme: surfacePattern.color is not a colour, using {PATTERN_COLOR}");
        return Ok(PATTERN_COLOR.to_string());
    };
    Ok(raw)
}

/// A kind this launcher does not know is no pattern, not a broken theme.
/// Serde's `other` attribute only works on tagged enums, so the mapping is
/// written out here.
fn kind_or_flat<'de, D>(deserializer: D) -> Result<SurfacePatternKind, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let LooseText::Text(raw) = LooseText::deserialize(deserializer)? else {
        log::warn!("theme: surfacePattern.kind is not a name, surfaces stay flat");
        return Ok(SurfacePatternKind::None);
    };
    Ok(match raw.trim().to_ascii_lowercase().as_str() {
        "none" => SurfacePatternKind::None,
        "scanlines" => SurfacePatternKind::Scanlines,
        "grid" => SurfacePatternKind::Grid,
        "dots" => SurfacePatternKind::Dots,
        "diagonal" => SurfacePatternKind::Diagonal,
        "gradient" => SurfacePatternKind::Gradient,
        other => {
            log::warn!("theme: `{other}` is not a surface pattern, surfaces stay flat");
            SurfacePatternKind::None
        }
    })
}

impl SurfacePattern {
    /// The `background-image` and the `background-size` for it, or `None` when
    /// the theme asks for nothing (or for something with an unusable colour —
    /// the figure would be invisible or, worse, carry a rule of its own).
    ///
    /// Every number is clamped here rather than trusted: this is the last
    /// place before the value stands in a declaration.
    pub fn to_css(&self) -> Option<(String, String)> {
        let color = self.color.trim();
        if self.kind == SurfacePatternKind::None {
            return None;
        }
        if !is_safe_css_color(color) {
            log::warn!("theme: `{color}` is not a colour, surfaces stay flat");
            return None;
        }
        let size = self
            .size
            .clamp(*PATTERN_SIZE_RANGE.start(), *PATTERN_SIZE_RANGE.end());
        let angle = self.angle % 360;
        // One hairline at `deg`, then nothing until the next one.
        let lines = |deg: u32| {
            format!(
                "repeating-linear-gradient({deg}deg, {color} 0, {color} 1px, transparent 1px, transparent {size}px)"
            )
        };
        Some(match self.kind {
            SurfacePatternKind::None => unreachable!("handled above"),
            SurfacePatternKind::Scanlines => (lines(0), "auto".into()),
            SurfacePatternKind::Grid => (format!("{}, {}", lines(0), lines(90)), "auto".into()),
            // A dot needs the size on the box, not in the figure: the radial
            // gradient draws one dot per tile of `background-size`.
            SurfacePatternKind::Dots => (
                format!("radial-gradient({color} 1px, transparent 1px)"),
                format!("{size}px {size}px"),
            ),
            SurfacePatternKind::Diagonal => (lines(angle), "auto".into()),
            SurfacePatternKind::Gradient => (
                format!("linear-gradient({angle}deg, {color}, transparent)"),
                "auto".into(),
            ),
        })
    }
}

/// One font file of the event, as `@font-face` needs it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct FontFace {
    /// The family name `font_family` refers to.
    pub family: String,
    /// The file: `http(s)` or a `data:` URI.
    pub src: String,
    /// `400`, `700`, `normal`, `bold` — optional.
    pub weight: Option<String>,
    /// `normal`, `italic`, `oblique` — optional.
    pub style: Option<String>,
}

impl FontFace {
    /// A family name that can stand between quotes in CSS trouble-free.
    /// Deliberately the same set as `FONT_FAMILY` in `src/lib/theme.ts`: a
    /// face the backend accepts and the UI then drops is a font that never
    /// arrives and nobody hears about.
    fn family_ok(&self) -> bool {
        let f = self.family.trim();
        !f.is_empty()
            && f.chars().count() <= 64
            && f.chars()
                .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '.' | '-' | '_'))
    }

    /// The file. The value is written into `url("…")`, so nothing that could
    /// leave those quotes may be in it — for the `data:` form that means an
    /// explicit character set, not just the prefix.
    fn src_ok(&self) -> bool {
        let s = self.src.trim();
        if let Some(data) = s.strip_prefix("data:font/") {
            return s.len() <= 2_000_000
                && !data.is_empty()
                && data.chars().all(|c| {
                    c.is_ascii_alphanumeric()
                        || matches!(c, ';' | ',' | '/' | '+' | '=' | '-' | '.' | '_')
                });
        }
        is_web_url(s)
    }

    fn weight_ok(&self) -> bool {
        match self.weight.as_deref().map(str::trim) {
            None => true,
            Some(w) => {
                matches!(w, "normal" | "bold" | "lighter" | "bolder")
                    || (w.len() <= 3
                        && w.starts_with(|c: char| c.is_ascii_digit() && c != '0')
                        && w.chars().all(|c| c.is_ascii_digit()))
            }
        }
    }

    fn style_ok(&self) -> bool {
        matches!(
            self.style.as_deref().map(str::trim),
            None | Some("normal") | Some("italic") | Some("oblique")
        )
    }

    pub fn is_valid(&self) -> bool {
        self.family_ok() && self.src_ok() && self.weight_ok() && self.style_ok()
    }

    /// As written down, before the address has been resolved: a LANPage names
    /// the file next to its `launcher.ini`, not a URL. What is still relative
    /// afterwards is dropped by [`Theme::normalise_for`].
    pub fn is_declared_ok(&self) -> bool {
        self.family_ok()
            && (self.src_ok() || is_relative_ref(self.src.trim(), FONT_EXTENSIONS))
            && self.weight_ok()
            && self.style_ok()
    }

    /// The rule for this file. Every part was checked first, so nothing here
    /// can close the block and start one of its own.
    pub fn to_css(&self) -> String {
        let mut css = format!(
            "@font-face {{ font-family: \"{}\"; src: url(\"{}\");",
            self.family.trim(),
            self.src.trim()
        );
        if let Some(w) = self.weight.as_deref().map(str::trim) {
            css.push_str(&format!(" font-weight: {w};"));
        }
        if let Some(st) = self.style.as_deref().map(str::trim) {
            css.push_str(&format!(" font-style: {st};"));
        }
        css.push_str(" font-display: swap; }");
        css
    }
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
            background_overlay: None,
            radius: 12,
            surface_pattern: None,
            font_family: None,
            heading_font_family: None,
            font_faces: Vec::new(),
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
            header: None,
            header_text: None,
            footer: None,
            footer_text: None,
        }
    }
}

impl Theme {
    pub fn parse(json: &str) -> crate::Result<Self> {
        Self::parse_at(json, None)
    }

    /// Parse a theme served by a LANPage. `base` is the address it came from,
    /// so `"logo": "logo.png"` — how anyone would write it in a file that
    /// sits next to the image — points at the LANPage instead of at nothing.
    pub fn parse_at(json: &str, base: Option<&str>) -> crate::Result<Self> {
        let mut theme: Theme = serde_json::from_str(json)?;
        if let Some(base) = base {
            theme.resolve_relative(base);
        }
        theme.drop_unusable();
        theme.validate()?;
        Ok(theme)
    }

    /// Make relative image and font addresses absolute against the LANPage
    /// they were served from. Anything already absolute stays as it is.
    /// Resolve the addresses against the LANPage and drop what is still not
    /// usable afterwards. This is what a fetched theme goes through.
    pub fn normalise_for(&mut self, base: &str) {
        self.resolve_relative(base);
        self.drop_unusable();
    }

    pub fn resolve_relative(&mut self, base: &str) {
        let Ok(base) = url::Url::parse(base) else {
            return;
        };
        let resolve = |v: &mut String, extensions: &[&str]| {
            let raw = v.trim().to_string();
            // A scheme means it is already an address of its own (`http:`,
            // `data:`); everything else has to look like a file next to the
            // theme, or `"logo": "none"` would become an address and render
            // as a broken image instead of being dropped.
            if raw.is_empty() || raw.contains(':') || !is_relative_ref(&raw, extensions) {
                return;
            }
            match base.join(&raw) {
                Ok(absolute) => *v = absolute.to_string(),
                Err(e) => log::warn!("theme: {raw} is not a usable address ({e})"),
            }
        };
        for v in [&mut self.logo, &mut self.background_image]
            .into_iter()
            .flatten()
        {
            resolve(v, IMAGE_EXTENSIONS);
        }
        for face in &mut self.font_faces {
            resolve(&mut face.src, FONT_EXTENSIONS);
        }
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
            // The bars of the launcher, so an ini-only LANPage can match its
            // own header band without hosting a theme.json.
            for (key, slot) in [
                ("theme_header", &mut c.header),
                ("theme_header_text", &mut c.header_text),
                ("theme_footer", &mut c.footer),
                ("theme_footer_text", &mut c.footer_text),
            ] {
                let Some(value) = extra.get(key).map(|v| v.trim()) else {
                    continue;
                };
                if is_safe_css_color(value) {
                    *slot = Some(value.to_string());
                    any = true;
                } else {
                    log::warn!("launcher.ini: {key} is not a colour: {value}");
                }
            }
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
        if let Some(value) = extra.get("theme_background_overlay").map(|v| v.trim()) {
            if is_safe_css_color(value) {
                theme.background_overlay = Some(value.to_string());
                any = true;
            } else {
                log::warn!("launcher.ini: theme_background_overlay is not a colour: {value}");
            }
        }
        for (key, slot) in [
            ("theme_background_image", &mut theme.background_image),
            ("theme_logo", &mut theme.logo),
        ] {
            let Some(value) = extra.get(key).map(|v| v.trim()) else {
                continue;
            };
            // A LANPage writes `logo.png`, meaning the file next to its own
            // `launcher.ini`; the fetch resolves that against the page.
            if is_image_ref(value) || is_relative_ref(value, IMAGE_EXTENSIONS) {
                *slot = Some(value.to_string());
                any = true;
            } else {
                log::warn!("launcher.ini: {key} is not a usable image address: {value}");
            }
        }
        // Body and headings, each a stack and the file for it. A font stack
        // reaches CSS as is, so it may only name families.
        for (family_key, slot) in [
            ("theme_font_family", &mut theme.font_family),
            ("theme_heading_font_family", &mut theme.heading_font_family),
        ] {
            let Some(value) = extra
                .get(family_key)
                .map(|v| v.trim())
                .filter(|v| !v.is_empty() && v.len() <= 200)
            else {
                continue;
            };
            if value.contains([';', '{', '}', '<', '>', '(', ')']) {
                log::warn!("launcher.ini: {family_key} is not a font stack: {value}");
                continue;
            }
            *slot = Some(value.to_string());
            any = true;
        }
        // One font file per stack: `theme_font_src` is the file, the family
        // comes from `theme_font_family`. A LANPage with more than one weight
        // per family is past what an ini can carry and wants a theme.json.
        for (src_key, family_key, stack) in [
            (
                "theme_font_src",
                "theme_font_family",
                theme.font_family.clone(),
            ),
            (
                "theme_heading_font_src",
                "theme_heading_font_family",
                theme.heading_font_family.clone(),
            ),
        ] {
            let Some(src) = extra.get(src_key).map(|v| v.trim()) else {
                continue;
            };
            let Some(stack) = stack else {
                log::warn!("launcher.ini: {src_key} without {family_key}, ignored: {src}");
                continue;
            };
            let face = FontFace {
                // `"Bebas Neue", sans-serif` is how a font stack is written;
                // the family behind it is what @font-face needs.
                family: stack
                    .split(',')
                    .next()
                    .unwrap_or(&stack)
                    .trim()
                    .trim_matches(['"', '\''])
                    .to_string(),
                src: src.to_string(),
                weight: None,
                style: None,
            };
            if !face.is_declared_ok() {
                log::warn!(
                    "launcher.ini: {src_key} is not a usable font file for `{}`: {src}",
                    face.family
                );
                continue;
            }
            // One family, one file. The ini gives neither face a weight, so a
            // second rule under the same name would claim the same family and
            // win by being later — the body would end up in the headline face,
            // which is the one thing this key exists to avoid. The same file
            // named twice is no conflict, just nothing left to add.
            if let Some(known) = theme
                .font_faces
                .iter()
                .find(|f| f.family.eq_ignore_ascii_case(&face.family))
            {
                if known.src != face.src {
                    log::warn!(
                        "launcher.ini: {src_key} gives `{}` a second file, ignored: {src}",
                        face.family
                    );
                }
                continue;
            }
            theme.font_faces.push(face);
            any = true;
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
        // The texture of the surfaces: the figure, and the three values that
        // describe it. Naming only the figure is the common case and enough.
        if let Some(raw) = extra.get("theme_surface_pattern").map(|k| k.trim()) {
            let kind = match raw.to_ascii_lowercase().as_str() {
                "none" => Some(SurfacePatternKind::None),
                "scanlines" => Some(SurfacePatternKind::Scanlines),
                "grid" => Some(SurfacePatternKind::Grid),
                "dots" => Some(SurfacePatternKind::Dots),
                "diagonal" => Some(SurfacePatternKind::Diagonal),
                "gradient" => Some(SurfacePatternKind::Gradient),
                _ => None,
            };
            match kind {
                None => log::warn!("launcher.ini: theme_surface_pattern is not a pattern: {raw}"),
                Some(kind) => {
                    let mut pattern = SurfacePattern {
                        kind,
                        ..SurfacePattern::default()
                    };
                    if let Some(value) = extra.get("theme_surface_pattern_color").map(|v| v.trim())
                    {
                        if is_safe_css_color(value) {
                            pattern.color = value.to_string();
                        } else {
                            log::warn!(
                                "launcher.ini: theme_surface_pattern_color is not a colour: {value}"
                            );
                        }
                    }
                    if let Some(raw) = extra.get("theme_surface_pattern_size").map(|v| v.trim()) {
                        match raw.parse::<u32>() {
                            Ok(size) if PATTERN_SIZE_RANGE.contains(&size) => pattern.size = size,
                            // Outside the range the default stands, and the
                            // log says why: `size = 1` is a solid block, not
                            // a figure anyone meant to ask for.
                            _ => log::warn!(
                                "launcher.ini: theme_surface_pattern_size is not a number between {} and {}: {raw}",
                                PATTERN_SIZE_RANGE.start(),
                                PATTERN_SIZE_RANGE.end()
                            ),
                        }
                    }
                    if let Some(raw) = extra.get("theme_surface_pattern_angle").map(|v| v.trim()) {
                        match raw.parse::<i64>() {
                            // A turn backwards is a direction too, and it has
                            // to mean here what it means in a theme.json.
                            Ok(angle) => pattern.angle = angle.rem_euclid(360) as u32,
                            Err(_) => log::warn!(
                                "launcher.ini: theme_surface_pattern_angle is not a number: {raw}"
                            ),
                        }
                    }
                    theme.surface_pattern = Some(pattern);
                    // `none` asks for no figure, which is what the surfaces
                    // do anyway: a page that says only that has named no
                    // theme, and must not replace the scheme the user chose.
                    any = any || kind != SurfacePatternKind::None;
                }
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
        let optional = [
            &self.colors.header,
            &self.colors.header_text,
            &self.colors.footer,
            &self.colors.footer_text,
            &self.background_overlay,
        ];
        // The pattern's ink ends up in a `background-image` the launcher
        // writes, so it is checked like every other colour of the theme — but
        // only for a figure that is actually drawn. A theme naming a kind only
        // a newer launcher knows would otherwise still lose everything here,
        // over a colour this build never paints with.
        let pattern = self
            .surface_pattern
            .as_ref()
            .filter(|p| p.kind != SurfacePatternKind::None)
            .map(|p| &p.color);
        for c in all
            .into_iter()
            .chain(optional.into_iter().flatten())
            .chain(pattern)
        {
            if !is_safe_css_color(c) {
                return Err(crate::Error::Settings(format!(
                    "invalid colour `{c}` in theme"
                )));
            }
        }
        if self.radius > 48 {
            return Err(crate::Error::Settings("radius must be <= 48".into()));
        }
        // The pattern's numbers are not checked here: they cannot carry a rule
        // (a `u32` is digits), and both the parser and `SurfacePattern::to_css`
        // bring them into range. Refusing a theme over two pixels would be the
        // harsher answer to the smaller problem.
        Ok(())
    }

    /// Drop what the launcher cannot use instead of refusing the theme: a
    /// relative `logo` has no base to resolve against here, but the colours
    /// around it are still good. Called after parsing, logged once.
    fn drop_unusable(&mut self) {
        self.font_faces.retain(|face| {
            let ok = face.is_valid();
            if !ok {
                log::warn!(
                    "theme: `{}` is not a usable font file, ignored",
                    face.family
                );
            }
            ok
        });
        self.drop_unusable_images();
    }

    fn drop_unusable_images(&mut self) {
        for (name, value) in [
            ("logo", &mut self.logo),
            ("backgroundImage", &mut self.background_image),
        ] {
            if let Some(v) = value {
                if !is_image_ref(v) {
                    log::warn!("theme: {name} is not an http(s) or data:image URL, ignored: {v}");
                    *value = None;
                }
            }
        }
    }

    /// The `@font-face` rules for the event's fonts, written by the launcher
    /// from the checked declarations.
    pub fn font_face_css(&self) -> Option<String> {
        let css: Vec<String> = self
            .font_faces
            .iter()
            .filter(|f| f.is_valid())
            .map(|f| f.to_css())
            .collect();
        (!css.is_empty()).then(|| css.join("\n"))
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
        // The chrome falls back to the surface rather than to a colour of its
        // own: a theme that says nothing about the bars keeps looking the way
        // it did before these keys existed.
        // Text on a bar the theme coloured: naming only the bar gets the ink
        // that reads on it, the way `primary` does.
        let ink_for = |named: Option<&String>, bar: Option<&String>, fallback: &str| -> String {
            match (named, bar) {
                (Some(text), _) => text.clone(),
                (None, Some(bar)) => readable_on(bar).unwrap_or(fallback).to_string(),
                (None, None) => fallback.to_string(),
            }
        };
        let header_text = ink_for(c.header_text.as_ref(), c.header.as_ref(), &c.text);
        let footer_text = ink_for(c.footer_text.as_ref(), c.footer.as_ref(), &c.text_muted);
        push("color-header", c.header.as_deref().unwrap_or(&c.surface));
        push("color-header-text", &header_text);
        push("color-footer", c.footer.as_deref().unwrap_or(&c.surface));
        push("color-footer-text", &footer_text);
        push("radius", &format!("{}px", self.radius));
        // Nothing is pushed for a flat theme, so the cards keep the plain
        // surface colour they had before this key existed.
        if let Some((image, size)) = self.surface_pattern.as_ref().and_then(|p| p.to_css()) {
            push("surface-pattern", &image);
            push("surface-pattern-size", &size);
        }
        if let Some(overlay) = &self.background_overlay {
            push("bg-overlay", overlay);
        }
        // A stack ends up in a declaration of its own, so nothing that could
        // close it may be in it. `theme.json` is not checked for this on the
        // way in — a stack that carried a rule would cost the whole theme
        // instead of just the font.
        let stack = |f: &String| -> String {
            f.chars()
                .filter(|ch| !matches!(ch, ';' | '{' | '}' | '<' | '>'))
                .collect()
        };
        if let Some(f) = &self.font_family {
            push("font-family", &stack(f));
        }
        // The headings fall back to the body font in CSS, so a theme that says
        // nothing about them keeps looking the way it did before this key.
        if let Some(f) = &self.heading_font_family {
            push("font-family-heading", &stack(f));
        }
        css
    }
}

/// A file named next to the LANPage's own files, which
/// [`Theme::resolve_relative`] turns into an address. No scheme, none of the
/// characters that could leave the `url("…")` it ends up in, and one of the
/// given extensions: a typo that is silently turned into an address only
/// shows up later as a broken image.
pub fn is_relative_ref(value: &str, extensions: &[&str]) -> bool {
    let v = value.trim();
    let lower = v.to_ascii_lowercase();
    !v.is_empty()
        && v.len() <= 512
        && !v.contains(':')
        && !v.contains(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '\\'))
        && extensions.iter().any(|ext| lower.ends_with(ext))
}

/// Extensions the launcher shows as an image, and those a browser loads as a
/// font — the two sets a relative address may carry.
pub const IMAGE_EXTENSIONS: &[&str] = &[
    ".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg", ".ico", ".bmp", ".avif",
];
pub const FONT_EXTENSIONS: &[&str] = &[".woff2", ".woff", ".ttf", ".otf"];

/// An image the launcher can show: an `http(s)` address or a `data:image` URI.
pub fn is_image_ref(value: &str) -> bool {
    let v = value.trim();
    if let Some(data) = v.strip_prefix("data:image/") {
        // As for a font file: the value is written into `url("…")`, so the
        // character set is checked, not only the prefix — otherwise a newline
        // or a quote costs the image with nothing in the log.
        return v.len() <= 2_000_000
            && !data.is_empty()
            && data.chars().all(|c| {
                c.is_ascii_alphanumeric()
                    || matches!(c, ';' | ',' | '/' | '+' | '=' | '-' | '.' | '_')
            });
    }
    is_web_url(v)
}

/// An `http(s)` URL of a length a stylesheet link can carry. Everything the
/// theme sends to the browser as a URL goes through this; `javascript:` and
/// `data:` have no business in a LANPage's font.
pub fn is_web_url(value: &str) -> bool {
    let v = value.trim();
    v.len() <= 512
        && (v.starts_with("http://") || v.starts_with("https://"))
        // A URL of ours ends up inside `url("…")` or an `src` attribute; the
        // quote characters and the CSS escape have no place in one.
        && !v.contains(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '\\'))
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
    fn the_bars_fall_back_to_the_surface_when_a_theme_says_nothing() {
        let css = Theme::default().to_css_variables();
        let surface = ThemeColors::default().surface;
        assert!(
            css.contains(&format!("--color-header: {surface};")),
            "{css}"
        );
        assert!(
            css.contains(&format!("--color-footer: {surface};")),
            "{css}"
        );
        assert!(!css.contains("--bg-overlay"), "{css}");
    }

    #[test]
    fn a_lanpage_can_dress_the_bars_and_the_background_from_the_ini() {
        let theme = Theme::from_ini(&ini(&[
            ("theme_header", "#101820"),
            ("theme_header_text", "#ffcc00"),
            ("theme_footer", "rgb(16, 24, 32)"),
            ("theme_background_image", "http://launcher.lan/bg.jpg"),
            ("theme_background_overlay", "rgba(0,0,0,0.55)"),
            ("theme_font_family", "\"Bebas Neue\", sans-serif"),
            ("theme_font_src", "http://launcher.lan/bebas.woff2"),
        ]))
        .expect("a theme");
        theme.validate().expect("valid");
        assert_eq!(theme.colors.header.as_deref(), Some("#101820"));
        assert_eq!(theme.colors.header_text.as_deref(), Some("#ffcc00"));
        assert_eq!(theme.colors.footer.as_deref(), Some("rgb(16, 24, 32)"));
        // The footer text was not named, so it keeps the muted text colour.
        assert_eq!(theme.colors.footer_text, None);
        assert_eq!(
            theme.background_image.as_deref(),
            Some("http://launcher.lan/bg.jpg")
        );
        let css = theme.to_css_variables();
        assert!(css.contains("--color-header: #101820;"), "{css}");
        assert!(css.contains("--bg-overlay: rgba(0,0,0,0.55);"), "{css}");
        // The footer text was not named and the footer is dark, so the ink is
        // the light one rather than the muted body colour.
        assert!(css.contains("--color-footer-text: #ffffff;"), "{css}");
        assert!(
            css.contains("--font-family: \"Bebas Neue\", sans-serif;"),
            "{css}"
        );
        // The quotes belong to the stack, not to the family the file provides.
        let fonts = theme.font_face_css().expect("font css");
        assert!(fonts.contains("font-family: \"Bebas Neue\";"), "{fonts}");
        assert!(
            fonts.contains("url(\"http://launcher.lan/bebas.woff2\")"),
            "{fonts}"
        );
    }

    #[test]
    fn urls_and_font_stacks_from_the_ini_are_checked() {
        // A `javascript:` logo, a font stack carrying a CSS rule and a font
        // file that is not a file would all end up in the document.
        let theme = Theme::from_ini(&ini(&[
            ("theme_logo", "javascript:alert(1)"),
            ("theme_font_src", "file:///etc/passwd"),
            ("theme_font_family", "Arial; } body { display: none"),
            ("theme_primary", "#29b6f6"),
        ]))
        .expect("a theme");
        assert_eq!(theme.logo, None);
        assert!(theme.font_faces.is_empty());
        assert_eq!(theme.font_family, None);
        assert_eq!(theme.colors.primary, "#29b6f6");
    }

    #[test]
    fn a_theme_may_draw_a_figure_on_its_surfaces() {
        // The CRT look of a LANPage: hairlines every 4 px. A colour cannot
        // say this and a stylesheet is not on offer, so the theme names the
        // figure and the launcher draws it.
        let theme = Theme::parse(
            r#"{"surfacePattern":{"kind":"scanlines","color":"rgba(0, 212, 255, 0.06)","size":4}}"#,
        )
        .expect("a theme");
        let css = theme.to_css_variables();
        assert!(
            css.contains(
                "--surface-pattern: repeating-linear-gradient(0deg, rgba(0, 212, 255, 0.06) 0, \
                 rgba(0, 212, 255, 0.06) 1px, transparent 1px, transparent 4px);"
            ),
            "{css}"
        );
        assert!(css.contains("--surface-pattern-size: auto;"), "{css}");
        // A dot needs its distance on the box, not in the figure.
        let dots = Theme::parse(r#"{"surfacePattern":{"kind":"dots","size":6}}"#).unwrap();
        let css = dots.to_css_variables();
        assert!(css.contains("--surface-pattern: radial-gradient("), "{css}");
        assert!(css.contains("--surface-pattern-size: 6px 6px;"), "{css}");
        // The grid draws both directions, the diagonal follows the angle.
        let grid = Theme::parse(r#"{"surfacePattern":{"kind":"grid"}}"#).unwrap();
        assert_eq!(
            grid.to_css_variables().matches("repeating-linear").count(),
            1 + 1
        );
        let diagonal =
            Theme::parse(r#"{"surfacePattern":{"kind":"diagonal","angle":135}}"#).unwrap();
        assert!(
            diagonal
                .to_css_variables()
                .contains("repeating-linear-gradient(135deg,"),
            "{diagonal:?}"
        );
        // Saying nothing leaves the surfaces flat, exactly as before the key.
        assert!(!Theme::default()
            .to_css_variables()
            .contains("--surface-pattern"));
        assert!(!Theme::parse(r#"{"surfacePattern":{"kind":"none"}}"#)
            .unwrap()
            .to_css_variables()
            .contains("--surface-pattern"));
    }

    #[test]
    fn a_pattern_cannot_carry_a_rule_of_its_own() {
        // Colour, figure and numbers all end up in a declaration the launcher
        // writes. The colour is checked like every other colour of the theme,
        // so a value that could close the declaration costs the theme.
        assert!(Theme::parse(
            r#"{"surfacePattern":{"kind":"scanlines","color":"red; } body { display: none"}}"#
        )
        .is_err());
        // A number cannot carry a rule, so it is brought into range instead of
        // costing the theme — and into the same range as in
        // `surfacePatternCss` of `src/lib/theme.ts`, which clamps as well.
        let css = |json: &str| Theme::parse(json).expect("a theme").to_css_variables();
        assert!(css(r#"{"surfacePattern":{"kind":"dots","size":0}}"#).contains(": 2px 2px;"));
        assert!(css(r#"{"surfacePattern":{"kind":"dots","size":800}}"#).contains(": 64px 64px;"));
        assert!(css(r#"{"surfacePattern":{"kind":"diagonal","angle":900}}"#).contains("(180deg,"));
        // A turn backwards is a direction too, and `"4"` is how a number
        // reaches a file somebody wrote by hand.
        assert!(css(r#"{"surfacePattern":{"kind":"diagonal","angle":-45}}"#).contains("(315deg,"));
        assert!(css(r#"{"surfacePattern":{"kind":"dots","size":"6"}}"#).contains(": 6px 6px;"));
        // And what is no number at all keeps the default. The ink is the same
        // slip and gets the same answer — only a colour that is a word but no
        // colour is refused, because that one could carry a rule.
        assert!(css(r#"{"surfacePattern":{"kind":"dots","size":null}}"#).contains(": 4px 4px;"));
        assert!(
            css(r#"{"surfacePattern":{"kind":"scanlines","color":null}}"#).contains(PATTERN_COLOR)
        );
        // A figure this launcher does not know is no pattern and no error: a
        // LANPage may name one a newer build draws.
        let unknown = Theme::parse(
            r##"{"surfacePattern":{"kind":"hexagons"},"colors":{"primary":"#29b6f6"}}"##,
        )
        .expect("still a theme");
        assert_eq!(unknown.colors.primary, "#29b6f6");
        assert!(!unknown.to_css_variables().contains("--surface-pattern"));
        // The same for a kind that is not even a word.
        assert!(!Theme::parse(r#"{"surfacePattern":{"kind":null}}"#)
            .expect("still a theme")
            .to_css_variables()
            .contains("--surface-pattern"));
        // And the ink of a figure nobody draws is nobody's problem: a page
        // naming a kind and a colour notation only a newer launcher knows
        // must not lose its theme here.
        let ahead = Theme::parse(
            r#"{"surfacePattern":{"kind":"hexagons","color":"oklch(0.7 0.1 200)"},
                "fontFamily":"Rajdhani"}"#,
        )
        .expect("still a theme");
        assert_eq!(ahead.font_family.as_deref(), Some("Rajdhani"));
        assert!(!ahead.to_css_variables().contains("--surface-pattern"));
    }

    #[test]
    fn the_ini_can_name_a_pattern_too() {
        let theme = Theme::from_ini(&ini(&[
            ("theme_surface_pattern", "Scanlines"),
            ("theme_surface_pattern_color", "rgba(0, 212, 255, 0.06)"),
            ("theme_surface_pattern_size", "4"),
        ]))
        .expect("a theme");
        theme.validate().expect("valid");
        let pattern = theme.surface_pattern.expect("a pattern");
        assert_eq!(pattern.kind, SurfacePatternKind::Scanlines);
        assert_eq!(pattern.size, 4);
        // Naming only the figure is the common case: the rest has defaults.
        let bare = Theme::from_ini(&ini(&[("theme_surface_pattern", "dots")]))
            .expect("a theme")
            .surface_pattern
            .expect("a pattern");
        assert_eq!(bare.color, PATTERN_COLOR);
        assert_eq!(bare.size, PATTERN_SIZE);
        // A value that is none of the figures, a colour that is no colour and
        // a size that is no number each cost themselves, not the theme.
        assert!(Theme::from_ini(&ini(&[("theme_surface_pattern", "sparkles")])).is_none());
        let sloppy = Theme::from_ini(&ini(&[
            ("theme_surface_pattern", "grid"),
            ("theme_surface_pattern_color", "red; } body { display: none"),
            ("theme_surface_pattern_size", "huge"),
            ("theme_surface_pattern_angle", "900"),
        ]))
        .expect("a theme");
        sloppy.validate().expect("valid");
        let pattern = sloppy.surface_pattern.expect("a pattern");
        assert_eq!(pattern.kind, SurfacePatternKind::Grid);
        assert_eq!(pattern.color, PATTERN_COLOR);
        assert_eq!(pattern.size, PATTERN_SIZE);
        // A direction wraps here as it does in a theme.json, rather than the
        // same value meaning two things in the two files.
        assert_eq!(pattern.angle, 900 % 360);
        let backwards = Theme::from_ini(&ini(&[
            ("theme_surface_pattern", "diagonal"),
            ("theme_surface_pattern_angle", "-45"),
        ]))
        .expect("a theme")
        .surface_pattern
        .expect("a pattern");
        assert_eq!(backwards.angle, 315);
        // A size below the range is a solid block, not a figure: the ini says
        // so in the log and keeps the default.
        let dense = Theme::from_ini(&ini(&[
            ("theme_surface_pattern", "scanlines"),
            ("theme_surface_pattern_size", "1"),
        ]))
        .expect("a theme")
        .surface_pattern
        .expect("a pattern");
        assert_eq!(dense.size, PATTERN_SIZE);
    }

    #[test]
    fn the_headings_may_wear_a_font_of_their_own() {
        // A LANPage puts a display face on its headlines and a readable one
        // on everything else; one stack for the whole UI can carry only the
        // second of those.
        let theme = Theme::parse(
            r#"{"fontFamily":"Rajdhani, sans-serif","headingFontFamily":"Alfa Slab One, serif",
                "fontFaces":[{"family":"Rajdhani","src":"http://launcher.lan/r.woff2"},
                             {"family":"Alfa Slab One","src":"http://launcher.lan/a.woff2"}]}"#,
        )
        .expect("a theme");
        let css = theme.to_css_variables();
        assert!(
            css.contains("--font-family: Rajdhani, sans-serif;"),
            "{css}"
        );
        assert!(
            css.contains("--font-family-heading: Alfa Slab One, serif;"),
            "{css}"
        );
        let fonts = theme.font_face_css().expect("font css");
        assert!(fonts.contains("font-family: \"Alfa Slab One\";"), "{fonts}");
        // Saying nothing about the headings leaves the property out; the UI
        // then falls back to the body font.
        assert!(!Theme::default()
            .to_css_variables()
            .contains("--font-family-heading"));
    }

    #[test]
    fn the_ini_can_name_the_heading_font_too() {
        let theme = Theme::from_ini(&ini(&[
            ("theme_font_family", "Rajdhani, sans-serif"),
            ("theme_font_src", "http://launcher.lan/r.woff2"),
            ("theme_heading_font_family", "\"Alfa Slab One\", serif"),
            ("theme_heading_font_src", "http://launcher.lan/a.woff2"),
        ]))
        .expect("a theme");
        assert_eq!(
            theme.heading_font_family.as_deref(),
            Some("\"Alfa Slab One\", serif")
        );
        let fonts = theme.font_face_css().expect("font css");
        assert!(
            fonts.contains("url(\"http://launcher.lan/r.woff2\")"),
            "{fonts}"
        );
        assert!(fonts.contains("font-family: \"Alfa Slab One\";"), "{fonts}");
        // Both stacks on the same file is one rule, not two.
        let same = Theme::from_ini(&ini(&[
            ("theme_font_family", "LAN"),
            ("theme_font_src", "http://launcher.lan/lan.woff2"),
            ("theme_heading_font_family", "LAN"),
            ("theme_heading_font_src", "http://launcher.lan/lan.woff2"),
        ]))
        .expect("a theme");
        assert_eq!(same.font_faces.len(), 1);
        // The same name for two different files is not two fonts: both rules
        // would claim the family and the later one would dress the whole UI
        // in the headline face. The first file keeps the name.
        let clash = Theme::from_ini(&ini(&[
            ("theme_font_family", "LAN"),
            ("theme_font_src", "http://launcher.lan/lan-text.woff2"),
            ("theme_heading_font_family", "LAN"),
            (
                "theme_heading_font_src",
                "http://launcher.lan/lan-display.woff2",
            ),
        ]))
        .expect("a theme");
        assert_eq!(clash.font_faces.len(), 1);
        assert_eq!(
            clash.font_faces[0].src,
            "http://launcher.lan/lan-text.woff2"
        );
        // A heading file without a heading family has no family to attach to
        // and must not silently borrow the body one.
        let orphan = Theme::from_ini(&ini(&[
            ("theme_font_family", "LAN"),
            ("theme_heading_font_src", "http://launcher.lan/a.woff2"),
        ]))
        .expect("a theme");
        assert!(orphan.font_faces.is_empty());
        // And a heading stack carrying a rule is dropped like the body one.
        let bad = Theme::from_ini(&ini(&[
            ("theme_heading_font_family", "Arial; } body { display: none"),
            ("theme_primary", "#29b6f6"),
        ]))
        .expect("a theme");
        assert_eq!(bad.heading_font_family, None);
    }

    #[test]
    fn one_font_file_fits_in_the_ini() {
        let theme = Theme::from_ini(&ini(&[
            ("theme_font_family", "Bebas Neue, sans-serif"),
            ("theme_font_src", "http://launcher.lan/bebas.woff2"),
        ]))
        .expect("a theme");
        let css = theme.font_face_css().expect("font css");
        assert!(css.contains("font-family: \"Bebas Neue\""), "{css}");
        assert!(
            css.contains("url(\"http://launcher.lan/bebas.woff2\")"),
            "{css}"
        );
    }

    #[test]
    fn a_font_file_becomes_a_rule_the_launcher_writes_itself() {
        let theme = Theme::parse(
            r#"{"fontFamily":"LAN, sans-serif","fontFaces":[
                 {"family":"LAN","src":"http://launcher.lan/lan.woff2","weight":"700"}]}"#,
        )
        .expect("a theme");
        let css = theme.font_face_css().expect("font css");
        assert!(css.contains("font-family: \"LAN\""), "{css}");
        assert!(
            css.contains("url(\"http://launcher.lan/lan.woff2\")"),
            "{css}"
        );
        assert!(css.contains("font-weight: 700"), "{css}");
    }

    #[test]
    fn a_font_declaration_cannot_carry_a_rule_of_its_own() {
        // Everything the launcher writes into the document comes from these
        // four fields, so each one is checked before it is written — and a
        // face that fails costs itself, not the theme around it.
        let bad = |json: &str| {
            Theme::parse(json)
                .expect("still a theme")
                .font_face_css()
                .is_none()
        };
        assert!(bad(
            r#"{"fontFaces":[{"family":"a\"} body{display:none}","src":"http://l/a.woff2"}]}"#
        ));
        assert!(bad(
            r#"{"fontFaces":[{"family":"LAN","src":"javascript:alert(1)"}]}"#
        ));
        assert!(bad(r#"{"fontFaces":[{"family":"LAN","src":"lan.woff2"}]}"#));
        assert!(bad(
            r#"{"fontFaces":[{"family":"LAN","src":"http://l/a.woff2","weight":"400; } body{display:none"}]}"#
        ));
        assert!(bad(
            r#"{"fontFaces":[{"family":"LAN","src":"http://l/a.woff2","style":"italic; }"}]}"#
        ));
        // A `data:` font is checked character by character, not just by its
        // prefix: a quote is how a rule of one's own would start.
        assert!(bad(
            r#"{"fontFaces":[{"family":"LAN","src":"data:font/woff2;base64,AA\"); } body { display: none } @font-face { src: url(\"x"}]}"#
        ));
        assert!(bad(
            r#"{"fontFaces":[{"family":"LAN","src":"http://l/a.woff2","weight":"0"}]}"#
        ));
        // A `data:` font is fine: it carries no address to follow.
        assert!(Theme::parse(
            r#"{"fontFaces":[{"family":"LAN","src":"data:font/woff2;base64,AA"}]}"#
        )
        .is_ok());
    }

    #[test]
    fn a_file_next_to_the_lanpage_is_found() {
        // `"logo": "logo.png"` is how anyone writes it in a file that sits
        // next to the image; inside the launcher that address means nothing
        // until it is resolved against the page it came from.
        let theme = Theme::parse_at(
            r#"{"logo":"logo.png","backgroundImage":"img/bg.jpg",
                "fontFamily":"LAN","fontFaces":[{"family":"LAN","src":"fonts/lan.woff2"}]}"#,
            Some("http://192.168.0.5/theme.json"),
        )
        .expect("a theme");
        assert_eq!(theme.logo.as_deref(), Some("http://192.168.0.5/logo.png"));
        assert_eq!(
            theme.background_image.as_deref(),
            Some("http://192.168.0.5/img/bg.jpg")
        );
        let fonts = theme.font_face_css().expect("font css");
        assert!(
            fonts.contains("url(\"http://192.168.0.5/fonts/lan.woff2\")"),
            "{fonts}"
        );
        // An address that is already one stays untouched.
        let theme = Theme::parse_at(
            r#"{"logo":"http://other.lan/l.png"}"#,
            Some("http://192.168.0.5/theme.json"),
        )
        .unwrap();
        assert_eq!(theme.logo.as_deref(), Some("http://other.lan/l.png"));
    }

    #[test]
    fn the_ini_may_name_the_file_next_to_it() {
        let mut theme = Theme::from_ini(&ini(&[
            ("theme_logo", "logo.png"),
            ("theme_font_family", "LAN"),
            ("theme_font_src", "lan.woff2"),
        ]))
        .expect("a theme");
        theme.normalise_for("http://192.168.0.5/launcher.ini");
        assert_eq!(theme.logo.as_deref(), Some("http://192.168.0.5/logo.png"));
        assert!(theme
            .font_face_css()
            .expect("font css")
            .contains("http://192.168.0.5/lan.woff2"));
    }

    #[test]
    fn a_relative_address_has_to_look_like_a_file() {
        // Resolving anything at all against the LANPage would turn a typo
        // into an address and show it as a broken image.
        let theme = Theme::from_ini(&ini(&[
            ("theme_logo", "logo"),
            ("theme_primary", "#29b6f6"),
        ]))
        .expect("a theme");
        assert_eq!(theme.logo, None);
        let theme = Theme::from_ini(&ini(&[("theme_logo", "bilder/logo.PNG")])).expect("a theme");
        assert_eq!(theme.logo.as_deref(), Some("bilder/logo.PNG"));
    }

    #[test]
    fn an_image_the_launcher_cannot_fetch_is_dropped_not_fatal() {
        // A relative path has no base inside the launcher. Dropping it costs
        // the image; refusing the file would cost the colours as well.
        let theme = Theme::parse(r#"{"logo":"logo.png","radius":8}"#).expect("still a theme");
        assert_eq!(theme.logo, None);
        assert_eq!(theme.radius, 8);
        assert_eq!(
            Theme::parse(r#"{"backgroundImage":"../bg.jpg"}"#)
                .unwrap()
                .background_image,
            None
        );
        assert!(Theme::parse(r#"{"logo":"http://launcher.lan/logo.png"}"#)
            .unwrap()
            .logo
            .is_some());
        assert!(Theme::parse(r#"{"logo":"data:image/png;base64,iVBOR"}"#)
            .unwrap()
            .logo
            .is_some());
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
