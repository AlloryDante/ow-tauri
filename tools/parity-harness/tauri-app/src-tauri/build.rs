//! Compiles the harness app's ACL. The app manifest is read at run time
//! (`PARITY_HARNESS_PACKAGE_JSON`), so the identity under test never enters
//! the build.

fn main() {
    tauri_build::build();
}
