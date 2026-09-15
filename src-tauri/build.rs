fn main() {
    // Short commit id so installers of the same version can be told apart in
    // the status bar and the log (CI sets GITHUB_SHA; local builds ask git).
    let build_id = std::env::var("GITHUB_SHA")
        .ok()
        .filter(|s| s.len() >= 7)
        .map(|s| s[..7].to_string())
        .or_else(|| {
            std::process::Command::new("git")
                .args(["rev-parse", "--short=7", "HEAD"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "dev".to_string());
    println!("cargo:rustc-env=NLL_BUILD_ID={build_id}");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    println!("cargo:rerun-if-env-changed=NLL_REQUIRE_ADMIN");
    // Re-run when the commit changes: HEAD itself plus the branch ref it points
    // to (HEAD only holds "ref: refs/heads/x" on a branch). Only existing
    // files are registered, otherwise Cargo would rebuild every time.
    let git_path = |what: &str| -> Option<String> {
        std::process::Command::new("git")
            .args(["rev-parse", "--git-path", what])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|p| std::path::Path::new(p).is_file())
    };
    let head_ref = std::process::Command::new("git")
        .args(["symbolic-ref", "-q", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    for what in ["HEAD", "packed-refs"]
        .into_iter()
        .map(str::to_string)
        .chain(head_ref)
    {
        if let Some(path) = git_path(&what) {
            println!("cargo:rerun-if-changed={path}");
        }
    }

    let mut attrs = tauri_build::Attributes::new();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // The launcher runs as the invoking user; setup scripts, firewall
        // rules and repairs are elevated on demand through UAC
        // (launch::elevate). NLL_REQUIRE_ADMIN=1 at build time restores the
        // ETI behaviour of one prompt at start.
        {
            let level = if std::env::var("NLL_REQUIRE_ADMIN").is_ok() {
                "requireAdministrator"
            } else {
                "asInvoker"
            };
            let manifest = format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*" />
    </dependentAssembly>
  </dependency>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="{level}" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>"#
            );
            attrs = attrs
                .windows_attributes(tauri_build::WindowsAttributes::new().app_manifest(manifest));
        }
    }
    tauri_build::try_build(attrs).expect("failed to run tauri-build");
}
