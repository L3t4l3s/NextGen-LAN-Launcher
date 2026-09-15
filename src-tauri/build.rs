fn main() {
    let mut attrs = tauri_build::Attributes::new();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // The ETI game scripts call `netsh advfirewall` and write to HKLM, so the
        // launcher runs elevated like the original. Set NLL_NO_ELEVATION=1 at
        // build time to produce a non-elevated binary.
        if std::env::var("NLL_NO_ELEVATION").is_err() {
            let manifest = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*" />
    </dependentAssembly>
  </dependency>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="requireAdministrator" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>"#;
            attrs = attrs
                .windows_attributes(tauri_build::WindowsAttributes::new().app_manifest(manifest));
        }
    }
    tauri_build::try_build(attrs).expect("failed to run tauri-build");
}
