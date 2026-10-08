fn main() {
    // Generate the app's own command ACL so a capability can grant
    // `allow-spike-marker` to window `main` (the SEC-B1 / B5 setup).
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(
            tauri_build::AppManifest::new().commands(&["spike_marker"]),
        ),
    )
    .expect("tauri_build");
}
