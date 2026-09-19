fn main() {
    tauri_build::build();
    embed_test_manifest();
}

/// `cargo test` builds its own binaries, and they never go through
/// `tauri_build`'s manifest embedding (`embed_resource`'s `compile()` only
/// links a resource into `[[bin]]` targets via `cargo:rustc-link-arg-bins`).
/// Without the Common Controls v6 manifest `mdm.exe` gets, Windows loads the
/// old system `comctl32.dll`, which lacks `SetWindowSubclass` /
/// `DefSubclassProc` / `TaskDialogIndirect` — imports pulled in by the dialog
/// plugin the moment a test binary reaches `commands::handler()` (as
/// `tests/commands.rs` does). The whole binary then fails to start at all
/// with STATUS_ENTRYPOINT_NOT_FOUND, before `main()` ever runs. Embed the
/// same manifest into test binaries too.
fn embed_test_manifest() {
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo for build scripts");
    let manifest_path = std::path::Path::new(&out_dir).join("test-manifest.xml");
    std::fs::write(&manifest_path, TEST_MANIFEST).expect("writing the test manifest");
    // Forward slashes: an .rc string literal would otherwise need the
    // Windows path's backslashes escaped.
    let manifest_path = manifest_path.display().to_string().replace('\\', "/");
    let rc_path = std::path::Path::new(&out_dir).join("test-manifest.rc");
    std::fs::write(&rc_path, format!("1 24 \"{manifest_path}\"\n"))
        .expect("writing the test manifest's .rc file");
    embed_resource::compile_for_tests(&rc_path, embed_resource::NONE)
        .manifest_required()
        .expect("embedding the Common Controls v6 manifest into test binaries");
}

/// Same content `tauri_build` embeds into `mdm.exe` (declares a dependency on
/// Common Controls v6, required by tray-icon / muda / the dialog plugin).
const TEST_MANIFEST: &str = r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*"
      />
    </dependentAssembly>
  </dependency>
</assembly>
"#;
