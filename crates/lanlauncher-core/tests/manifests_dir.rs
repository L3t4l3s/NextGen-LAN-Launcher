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
        // A manifest without an entry point is guidance only: no verified
        // executable, but setup notes worth showing. Everything else must name
        // what to start and what proves the extraction complete.
        if m.launch.exe.is_empty() {
            assert!(
                m.setup.notes.contains_key("de") && m.setup.notes.contains_key("en"),
                "{}: no launch.exe and no setup notes — nothing this manifest could add",
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
