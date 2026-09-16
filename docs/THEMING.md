# Theming

Organisers can brand the launcher per event without touching code.

## theme.json (served at `http://launcher.lan/theme.json` or referenced by `theme_url` in launcher.ini)

```json
{
  "version": 2,
  "name": "My LAN 2026",
  "mode": "dark",
  "colors": {
    "background": "#0f1218", "surface": "#171c25", "surfaceAlt": "#1f2632",
    "text": "#f2f4f8", "textMuted": "#9aa4b5",
    "primary": "#4f8cff", "primaryText": "#ffffff", "accent": "#7ee8ff",
    "success": "#2ecc71", "warning": "#f1c40f", "danger": "#ff5c5c", "border": "#2a3140",
    "header": "#0b2a45", "headerText": "#e8f4ff",
    "footer": "#06121f", "footerText": "#7f97af"
  },
  "logo": "http://launcher.lan/logo.png",
  "backgroundImage": "http://launcher.lan/bg.jpg",
  "backgroundOverlay": "rgba(4, 12, 24, 0.66)",
  "radius": 12,
  "fontFamily": "Bebas Neue, sans-serif",
  "fontFaces": [{ "family": "Bebas Neue", "src": "http://launcher.lan/bebas.woff2", "weight": "400" }],
  "icons": {}
}
```

The four `colors` keys for the bars, `backgroundOverlay`, `fontFamily` and `fontFaces` are what a
LANPage needs to look like itself inside the launcher:

| Key | Effect | Falls back to |
|---|---|---|
| `colors.header` / `colors.headerText` | the top bar with the logo and the tabs | `surface` / `text` |
| `colors.footer` / `colors.footerText` | the status bar at the bottom | `surface` / `textMuted` |
| `backgroundImage` | a picture behind the whole window | none |
| `backgroundOverlay` | a colour over that picture, so the text stays readable | transparent |
| `fontFamily` | the font stack of the whole UI | the system font |
| `fontFaces` | the font files themselves (`family`, `src`, optional `weight`/`style`) | none |

A LANPage names its font **files**, not a stylesheet: the launcher writes the `@font-face` rules
itself from `family`, `src`, `weight` and `style`, and each of those is checked first (a family is
letters, digits, spaces and dashes; `src` is `http(s)` or a `data:font/…` URI; the weight is a
number or `normal`/`bold`; the style is `normal`/`italic`/`oblique`). A LANPage therefore cannot
hand the launcher CSS — `button { display: none }` has nowhere to go. The font files themselves are
loaded by the browser, which is why the content-security policy allows fonts and images from any
`http(s)` address: a LANPage usually lives at an IP, not at `launcher.lan`. Remote stylesheets and
scripts stay blocked.

URLs must be `http(s)` — `logo` and `backgroundImage` may also be a `data:image/…` URI, but not a
relative path: the theme is applied inside the launcher, which has no idea where the file would sit.
`javascript:` and `file:` are rejected, in the file as in `launcher.ini`. A font stack containing anything but family names is ignored. So no theme can add a
rule to the document — the only exception stays the legacy `launcher.css` below, which was always an
ETI feature and is scoped to the background layer.

All fields are optional; missing values fall back to the default theme — including the colours,
so `"mode": "light"` on its own leaves the dark palette in place; name the colours you want. (The
`theme_mode` key of `launcher.ini` below does switch the palette, because there is no file to take
the colours from.) Colours are validated
(hex, rgb()/hsl(), names) so a theme can never inject CSS. Layout is deliberately not
themable so support instructions stay valid across LANs.

## Colours in `launcher.ini`

A LANPage that would rather add three lines to a file it already serves than host another one can
name its colours in `launcher.ini`, in the same block format as every other key:

```ini
theme_primary ### NextGen: accent colour of the launcher {
#29b6f6
}

theme_background ### NextGen: page background {
#0b1a2b
}
```

Recognised keys: `theme_mode` (`dark`/`light`, picks the base the rest is applied to),
`theme_name`, `theme_radius`, and one per colour — `theme_background`, `theme_surface`,
`theme_surface_alt`, `theme_text`, `theme_text_muted`, `theme_primary`, `theme_primary_text`,
`theme_accent`, `theme_success`, `theme_warning`, `theme_danger`, `theme_border`,
`theme_header`, `theme_header_text`, `theme_footer`, `theme_footer_text`.

The same goes for most of what `theme.json` can say: `theme_logo`, `theme_background_image`,
`theme_background_overlay`, `theme_font_family` and `theme_font_src` (one font file, using the
first family of `theme_font_family`; several weights want a `theme.json`). URLs must be `http(s)`,
and a value that is neither a colour nor a URL is ignored with a line in the log.

`NO_THEME_JSON=1 node tools/dev-lanpage/server.mjs` serves the dev LANPage without a theme.json,
which is how to try these keys locally.

Naming `theme_primary` without `theme_primary_text` gives the button label the ink that reads
better on it; saying both keeps exactly what you said.

Values are validated like every other colour, and a malformed one is ignored with a line in the
log instead of costing the whole theme. A served `theme.json` wins over these keys: it is the
deliberate one and can say more. Naming no colour at all (only `theme_name`) yields no theme, so a
LANPage cannot override a scheme the user picked in the settings without meaning to.

## Legacy `launcher.css`

The old ETI stylesheet (`html { background: … }`, `#bg_layer { … }`) is still fetched and
applied to the background layer only.

## Local themes

`themes/*.json` ship with the app; the user can pick one in Settings → Farbschema. "Automatisch"
uses the event theme when present.

## Built-in themes

`src/lib/theme.ts` (`builtinThemes`) ships NextGen Dark (`default`), NextGen Light (`light`) and
the colour schemes Blau (`blue`), Grün (`green`), Orange (`orange`), Pink (`pink`) and Rot
(`red`); the JSON files under `themes/` mirror them for organisers who want a starting point.
"Automatic" takes the LANPage's `launcher.css` and `logo.png`, a served `theme.json`, or the
`theme_*` keys of `launcher.ini`.

The ids of two schemes changed with their names (`unicorn` → `pink`, `beispiel-lan` → `orange`);
`Settings::migrate` carries an existing choice over.
