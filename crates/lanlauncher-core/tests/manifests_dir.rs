//! Every bundled manifest in `manifests/` must parse and be internally consistent.

use lanlauncher_core::manifest::Manifest;
use std::path::Path;

#[test]
fn bundled_manifests_are_valid() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../manifests");
    let mut count = 0;
    for entry in std::fs::read_dir(&dir).expect("manifests dir") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let m = Manifest::parse(&text, &path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let stem = path.file_stem().unwrap().to_string_lossy();
        assert_eq!(m.id, stem, "manifest id must match file name");
        // A manifest without an entry point starts through the game's own
        // script: no verified executable, but setup notes worth showing,
        // arguments for the script's programs, the player's settings,
        // Windows components or the claim that this start was checked.
        // Everything else must name what to start and what proves the
        // extraction complete.
        // Every note in both languages: the other one falls back to English.
        for by in std::iter::once(&m.setup.notes).chain(m.setup.platform_notes.values()) {
            assert!(
                by.is_empty() || (by.contains_key("de") && by.contains_key("en")),
                "{}: notes in German and English",
                path.display()
            );
        }
        if m.launch.exe.is_empty() {
            let both = |by: &std::collections::BTreeMap<String, String>| {
                by.contains_key("de") && by.contains_key("en")
            };
            let notes = both(&m.setup.notes) || m.setup.platform_notes.values().any(both);
            assert!(
                notes
                    || !m.script_args.is_empty()
                    || !m.settings.is_empty()
                    || !m.launch.winetricks.is_empty()
                    || !m.script_start_checked.is_empty(),
                "{}: no launch.exe, no setup notes, no script arguments, no settings, no components — \
                 nothing this manifest could add",
                path.display()
            );
        } else {
            assert!(
                !m.launch.required_files.is_empty(),
                "{}: required_files missing",
                path.display()
            );
            for platform in ["windows", "macos", "linux"] {
                let spec = m.launch_for(platform);
                assert!(!spec.exe.is_empty());
            }
        }
        count += 1;
    }
    assert!(count >= 6, "expected bundled manifests, found {count}");
}
