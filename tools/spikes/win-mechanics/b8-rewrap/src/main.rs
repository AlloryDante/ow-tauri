//! See Cargo.toml. On Windows this links two webview2-com-sys versions; the
//! build either succeeds (re-wrap feasible) or fails to link (collision).
//! On other OSes it is a no-op so the crate still checks locally.

#[cfg(windows)]
fn main() {
    use std::ffi::c_void;
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller as CtrlWry;
    use webview2_com_alt::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller as CtrlAlt;
    use windows_core::Interface as _;
    use windows_core_alt::Interface as _;

    // A real pointer is only available inside a running webview; this binary
    // proves the cross-minor re-wrap compiles and links. We exercise both
    // version sets on a null pointer (never dereferenced: from_raw_borrowed on
    // null returns None), which forces both webview2-com-sys loaders to link.
    let raw: *mut c_void = std::ptr::null_mut();

    // 0.39.1 (wry's) side: reference the type and its vtable call.
    let wry = unsafe { CtrlWry::from_raw_borrowed(&raw) };
    // 0.38.2 (plugin's own) side: re-wrap and call a method if non-null.
    let alt = unsafe { CtrlAlt::from_raw_borrowed(&raw) };
    let mut called_alt = false;
    if let Some(alt) = alt {
        // Not reached with a null pointer; present so the vtable call links.
        if let Ok(core) = unsafe { alt.CoreWebView2() } {
            let _ = unsafe { core.Settings() };
            called_alt = true;
        }
    }
    println!(
        "{{\"compiled\":true,\"linked\":true,\"wry_wrapped\":{},\"alt_wrapped\":{},\"alt_vtable_called\":{}}}",
        wry.is_some(),
        false,
        called_alt
    );
}

#[cfg(not(windows))]
fn main() {
    println!("{{\"unsupported\":\"windows-only (two webview2-com-sys versions)\"}}");
}
