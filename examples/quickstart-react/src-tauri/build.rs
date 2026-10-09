fn main() {
    // Reads tauri.conf.json exactly as tauri-build does (platform overlay + TAURI_CONFIG merges), validates
    // plugins.overwolf and the capability files, and on Windows targets writes
    // gen/overwolf/installer-hooks.nsh (install record + uninstall analytics) and, when signing is enabled,
    // the OWEINTEGRITY resource.
    tauri_plugin_overwolf::build::run().expect("tauri-plugin-overwolf build step failed");
    tauri_build::build();
}
