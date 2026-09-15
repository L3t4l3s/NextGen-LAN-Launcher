# Theming

Organisers can brand the launcher per event without touching code.

## theme.json (served at `http://launcher.lan/theme.json` or referenced by `theme_url` in launcher.ini)

```json
{
  "version": 1,
  "name": "My LAN 2026",
  "mode": "dark",
  "colors": {
    "background": "#0f1218", "surface": "#171c25", "surfaceAlt": "#1f2632",
    "text": "#f2f4f8", "textMuted": "#9aa4b5",
    "primary": "#4f8cff", "primaryText": "#ffffff", "accent": "#ff9f43",
    "success": "#2ecc71", "warning": "#f1c40f", "danger": "#ff5c5c", "border": "#2a3140"
  },
  "logo": "http://launcher.lan/logo.png",
  "backgroundImage": "http://launcher.lan/bg.jpg",
  "radius": 12,
  "fontFamily": null,
  "icons": {}
}
```

All fields are optional; missing values fall back to the default theme. Colours are validated
(hex, rgb()/hsl(), names) so a theme can never inject CSS. Layout is deliberately not
themable so support instructions stay valid across LANs.

## Legacy `launcher.css`

The old ETI stylesheet (`html { background: … }`, `#bg_layer { … }`) is still fetched and
applied to the background layer only.

## Local themes

`themes/*.json` ship with the app; the user can pick one in Settings → Farbschema. "Automatisch"
uses the event theme when present.

## Built-in themes

`src/lib/theme.ts` (`builtinThemes`) ships NextGen Dark (`default`), NextGen Light (`light`),
Pinkes Einhorn (`unicorn`) and Beispiel-LAN Orange (`beispiel-lan`); the JSON files under `themes/`
mirror them for organisers who want a starting point. "Automatic" takes the LANPage's
`launcher.css` and `logo.png` (or a served `theme.json`).
