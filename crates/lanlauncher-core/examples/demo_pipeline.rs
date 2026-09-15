//! Runs the demo transport through the install pipeline, like the app does.
//! `cargo run -p lanlauncher-core --release --example demo_pipeline`
use lanlauncher_core::catalog::{Catalog, Game, ShareKey};
use lanlauncher_core::install::{InstallManager, ManifestSetupHook};
use lanlauncher_core::paths::GamePaths;
use lanlauncher_core::transport::demo::DemoTransport;
use lanlauncher_core::transport::Transport;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let root = std::env::temp_dir().join("nll-demo-pipeline");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let mut demo = DemoTransport::new();
    demo.duration = Duration::from_secs(3);
    demo.stuck_at_99 = true;
    demo.archive_source = Some(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample_game.rar"),
    );
    let transport: Arc<dyn Transport> = Arc::new(demo);
    let mut catalog = Catalog::default();
    catalog.games.push(Game {
        id: "amongus".into(),
        order: 1,
        title: "Among Us".into(),
        key: ShareKey::parse("BAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA").unwrap(),
        revision: "20250308".into(),
        size_bytes: 4096,
        release_year: None,
        publisher: None,
        max_players: None,
        needs_master_server: false,
        genre_id: None,
        readme: Default::default(),
    });
    let r2 = root.clone();
    let mut manager = InstallManager::new(
        transport,
        catalog,
        move |g| Some(GamePaths::new(&r2, &g.id)),
        |_, _| None,
        Arc::new(ManifestSetupHook),
    );
    manager.policy.stable_for = Duration::from_secs(1);
    manager.install("amongus").await.unwrap();
    for i in 0..30 {
        let st = manager.tick().await;
        println!(
            "tick {i}: {:?}",
            st.iter().map(|s| (s.phase, s.progress)).collect::<Vec<_>>()
        );
        if st
            .iter()
            .any(|s| s.phase == lanlauncher_core::install::Phase::Ready)
        {
            println!("READY");
            return;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    println!("NOT READY");
}
