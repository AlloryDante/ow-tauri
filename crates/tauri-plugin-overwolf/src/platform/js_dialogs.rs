//! The page's `alert()`, `confirm()`, `prompt()` and `beforeunload` dialogs
//! in the webviews the plugin manages (`ow-main`, `bw-*`, `bwr-*` and ad
//! guests), shown as Electron shows them (CONTRACT B.2.6): `alert()` and
//! `confirm()` as a native message box with "OK" (and "Cancel"), attached to
//! the page's window while that window is visible; `prompt()` returns `null`
//! without a dialog; a `beforeunload` prompt keeps the page without a
//! dialog.
//!
//! macOS: wry's `WKUIDelegate` implements none of the panels, so `WebKit`
//! answers them itself (`alert()` returns at once, `confirm()` is `false`).
//! The plugin adds the alert and confirm panels to that delegate class.
//! Windows: WebView2's own dialogs are turned off and `ScriptDialogOpening`
//! shows a `MessageBoxW` instead. Linux keeps WebKitGTK's dialogs (PARITY,
//! known platform gaps).
//!
//! In the invisible lab no dialog is shown: the page gets the dismissed
//! answer and the lab records the dialog in `blocked.jsonl`.

#![cfg_attr(
    all(target_os = "linux", not(test)),
    expect(dead_code, reason = "Linux keeps WebKitGTK's dialogs")
)]

use serde_json::json;

/// A JavaScript dialog the page opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DialogKind {
    /// `alert()`.
    Alert,
    /// `confirm()`.
    Confirm,
    /// `prompt()`.
    #[cfg_attr(
        all(target_os = "macos", not(test)),
        expect(dead_code, reason = "only WebView2 reports a prompt")
    )]
    Prompt,
    /// A `beforeunload` handler that asks to stay on the page.
    #[cfg_attr(
        all(target_os = "macos", not(test)),
        expect(dead_code, reason = "only WebView2 reports a beforeunload prompt")
    )]
    BeforeUnload,
}

impl DialogKind {
    /// The name the lab records.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Alert => "alert",
            Self::Confirm => "confirm",
            Self::Prompt => "prompt",
            Self::BeforeUnload => "beforeunload",
        }
    }

    /// The buttons of the message box, first the one that accepts; empty
    /// when Electron shows no dialog.
    pub(crate) fn buttons(self) -> &'static [&'static str] {
        match self {
            Self::Alert => &["OK"],
            Self::Confirm => &["OK", "Cancel"],
            Self::Prompt | Self::BeforeUnload => &[],
        }
    }
}

/// How a dialog is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Presentation {
    /// Attached to the page's window (a sheet on macOS, an owned message box
    /// on Windows).
    Attached,
    /// On its own, because the page's window is hidden: a dialog attached to
    /// a hidden window could not be dismissed.
    Detached,
    /// Not shown: the invisible lab.
    Blocked,
    /// No dialog: the page gets the dismissed answer.
    None,
}

/// How a `kind` dialog is shown, given whether the invisible lab runs and
/// whether the page's window is visible.
pub(crate) fn presentation(kind: DialogKind, lab_invisible: bool, visible: bool) -> Presentation {
    if kind.buttons().is_empty() {
        Presentation::None
    } else if lab_invisible {
        Presentation::Blocked
    } else if visible {
        Presentation::Attached
    } else {
        Presentation::Detached
    }
}

/// Whether the page gets the accepting answer (`alert()` returns either
/// way; `confirm()` is `true`; the page leaves for `beforeunload`) when the
/// user pressed the button at `pressed` (`None`: no dialog, or dismissed).
pub(crate) fn accepted(kind: DialogKind, pressed: Option<usize>) -> bool {
    !kind.buttons().is_empty() && pressed == Some(0)
}

/// Records a dialog the invisible lab did not show. Returns whether it was
/// blocked.
#[cfg_attr(
    all(target_os = "linux", test),
    expect(dead_code, reason = "Linux keeps WebKitGTK's dialogs")
)]
fn block_in_lab(kind: DialogKind) -> bool {
    crate::lab::block_os_surface("js-dialog", || json!({ "type": kind.name() }))
}

/// Installs the dialogs for the `WKWebView` `wk_webview` (on the main
/// thread; see the module documentation).
#[cfg(target_os = "macos")]
pub(crate) fn install(wk_webview: *mut std::ffi::c_void) {
    macos::install(wk_webview);
}

/// Installs the dialogs for the webview of `controller` (on its thread;
/// see the module documentation). `app_name` is the message box caption;
/// `post` runs a task later on that thread.
#[cfg(windows)]
pub(crate) fn install(
    controller: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller,
    app_name: &str,
    post: Post,
) {
    if let Err(error) = windows_impl::install(controller, app_name, post) {
        log::debug!("JavaScript dialogs not installed: {error}");
    }
}

#[cfg(windows)]
pub(crate) use windows_impl::Post;

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::{CStr, c_void};

    use block2::{Block, RcBlock};
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Sel};
    use objc2::{class, msg_send, sel};
    use objc2_foundation::NSString;

    use super::{DialogKind, Presentation, accepted, block_in_lab, presentation};

    /// The name of wry's `WKUIDelegate` class contains this (the full name
    /// carries wry's module path and version).
    const DELEGATE_CLASS: &str = "WryWebViewUIDelegate";

    /// `v@:@@@@?`: `void (id self, SEL, WKWebView *, NSString *,
    /// WKFrameInfo *, void (^)(...))`.
    const PANEL_TYPES: &CStr = c"v@:@@@@?";

    /// `NSAlertFirstButtonReturn`.
    const FIRST_BUTTON: isize = 1000;

    type AlertPanel = extern "C-unwind" fn(
        &AnyObject,
        Sel,
        &AnyObject,
        &NSString,
        *mut AnyObject,
        &Block<dyn Fn()>,
    );
    type ConfirmPanel = extern "C-unwind" fn(
        &AnyObject,
        Sel,
        &AnyObject,
        &NSString,
        *mut AnyObject,
        &Block<dyn Fn(Bool)>,
    );

    extern "C-unwind" fn alert_panel(
        _this: &AnyObject,
        _cmd: Sel,
        web_view: &AnyObject,
        message: &NSString,
        _frame: *mut AnyObject,
        done: &Block<dyn Fn()>,
    ) {
        let done = done.copy();
        show(web_view, DialogKind::Alert, message, move |_| done.call(()));
    }

    extern "C-unwind" fn confirm_panel(
        _this: &AnyObject,
        _cmd: Sel,
        web_view: &AnyObject,
        message: &NSString,
        _frame: *mut AnyObject,
        done: &Block<dyn Fn(Bool)>,
    ) {
        let done = done.copy();
        show(web_view, DialogKind::Confirm, message, move |ok| {
            done.call((Bool::new(ok),));
        });
    }

    /// Shows `kind` for `web_view` and calls `answer` with whether it was
    /// accepted, once.
    fn show(
        web_view: &AnyObject,
        kind: DialogKind,
        message: &NSString,
        answer: impl Fn(bool) + 'static,
    ) {
        // SAFETY: `window` is a public `NSView` getter (nil when detached).
        let window: *mut AnyObject = unsafe { msg_send![web_view, window] };
        // SAFETY: `isVisible` is a public `NSWindow` getter.
        let visible = !window.is_null() && unsafe { msg_send![window, isVisible] };
        let how = presentation(kind, crate::lab::invisible(), visible);
        if matches!(how, Presentation::None)
            || (matches!(how, Presentation::Blocked) && block_in_lab(kind))
        {
            answer(accepted(kind, None));
            return;
        }
        // SAFETY: `+[NSAlert new]` on the main thread (a WebKit delegate
        // callback).
        let alert: Retained<AnyObject> = unsafe { msg_send![class!(NSAlert), new] };
        // SAFETY: public `NSAlert` setters with an `NSString`.
        unsafe {
            let () = msg_send![&alert, setMessageText: message];
            for title in kind.buttons() {
                let title = NSString::from_str(title);
                let _: *mut AnyObject = msg_send![&alert, addButtonWithTitle: &*title];
            }
        }
        let pressed = |response: isize| {
            usize::try_from(response - FIRST_BUTTON)
                .ok()
                .filter(|i| *i < kind.buttons().len())
        };
        if matches!(how, Presentation::Attached) {
            let handler = RcBlock::new(move |response: isize| {
                answer(accepted(kind, pressed(response)));
            });
            // SAFETY: a visible window and a completion block that lives as
            // long as AppKit keeps it.
            unsafe {
                let () = msg_send![&alert, beginSheetModalForWindow: window, completionHandler: &*handler];
            }
        } else {
            // SAFETY: a modal run of the alert on the main thread.
            let response: isize = unsafe { msg_send![&alert, runModal] };
            answer(accepted(kind, pressed(response)));
        }
    }

    /// Adds `imp` as `selector` to `class` unless the class has it already.
    fn add_panel(class: &AnyClass, selector: Sel, imp: Imp) {
        if class.instance_method(selector).is_some() {
            return;
        }
        // SAFETY: `imp` has the selector's signature, as `PANEL_TYPES` says.
        let _ = unsafe {
            objc2::ffi::class_addMethod(
                std::ptr::from_ref(class).cast_mut(),
                selector,
                imp,
                PANEL_TYPES.as_ptr(),
            )
        };
    }

    /// Adds the alert and confirm panels to wry's delegate of `wk_webview`
    /// and sets the delegate again, since `WebKit` reads which panels a
    /// delegate has when it is set. A webview whose delegate is not wry's
    /// is left as it is.
    pub(super) fn install(wk_webview: *mut c_void) {
        if wk_webview.is_null() {
            return;
        }
        // SAFETY: Tauri hands a live `WKWebView*` on the main thread.
        let web_view: &AnyObject = unsafe { &*wk_webview.cast::<AnyObject>() };
        // SAFETY: `UIDelegate` is a public `WKWebView` getter.
        let delegate: *mut AnyObject = unsafe { msg_send![web_view, UIDelegate] };
        if delegate.is_null() {
            return;
        }
        // SAFETY: a live delegate object; `class` reads its isa.
        let class = unsafe { &*delegate }.class();
        let wry = class.name().to_string_lossy().contains(DELEGATE_CLASS);
        crate::lab::record(
            "wc-events.jsonl",
            || serde_json::json!({ "kind": "js-dialogs", "installed": wry }),
        );
        if !wry {
            return;
        }
        let alert: AlertPanel = alert_panel;
        let confirm: ConfirmPanel = confirm_panel;
        // SAFETY: function pointers cast to the runtime's `Imp` type.
        let (alert, confirm) = unsafe {
            (
                std::mem::transmute::<AlertPanel, Imp>(alert),
                std::mem::transmute::<ConfirmPanel, Imp>(confirm),
            )
        };
        add_panel(
            class,
            sel!(webView:runJavaScriptAlertPanelWithMessage:initiatedByFrame:completionHandler:),
            alert,
        );
        add_panel(
            class,
            sel!(webView:runJavaScriptConfirmPanelWithMessage:initiatedByFrame:completionHandler:),
            confirm,
        );
        // SAFETY: the delegate the webview already has; wry keeps it alive.
        let () = unsafe { msg_send![web_view, setUIDelegate: delegate] };
    }
}

#[cfg(windows)]
mod windows_impl {
    use std::sync::Arc;

    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_SCRIPT_DIALOG_KIND, COREWEBVIEW2_SCRIPT_DIALOG_KIND_ALERT,
        COREWEBVIEW2_SCRIPT_DIALOG_KIND_CONFIRM, COREWEBVIEW2_SCRIPT_DIALOG_KIND_PROMPT,
        ICoreWebView2Controller, ICoreWebView2Deferral, ICoreWebView2ScriptDialogOpeningEventArgs,
    };
    use webview2_com::{ScriptDialogOpeningEventHandler, take_pwstr};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GA_ROOT, GetAncestor, IDOK, IsWindowVisible, MB_OK, MB_OKCANCEL, MessageBoxW,
    };
    use windows::core::{BOOL, HSTRING, PWSTR};

    use super::{DialogKind, Presentation, accepted, block_in_lab, presentation};

    /// Runs a task later on the webview's (the main) thread.
    pub(crate) type Post = Arc<dyn Fn(Box<dyn FnOnce() + Send>) + Send + Sync>;

    /// The event's arguments and deferral, used only on the webview's
    /// thread.
    struct Pending {
        args: ICoreWebView2ScriptDialogOpeningEventArgs,
        deferral: ICoreWebView2Deferral,
        owner: HWND,
        message: HSTRING,
        caption: HSTRING,
        kind: DialogKind,
    }

    // SAFETY: a `Pending` is created on the webview's thread and `Post`
    // runs it on that same thread (Tauri's main thread, where WebView2
    // lives); it never runs elsewhere.
    unsafe impl Send for Pending {}

    impl Pending {
        fn run(self) {
            let style = if self.kind == DialogKind::Confirm {
                MB_OKCANCEL
            } else {
                MB_OK
            };
            // SAFETY: a modal message box on the UI thread, outside any
            // WebView2 event handler.
            let pressed =
                unsafe { MessageBoxW(Some(self.owner), &self.message, &self.caption, style) };
            let pressed = (pressed == IDOK).then_some(0);
            // SAFETY: COM calls on the webview's thread.
            unsafe {
                if accepted(self.kind, pressed) {
                    let _ = self.args.Accept();
                }
                let _ = self.deferral.Complete();
            }
        }
    }

    fn kind_of(kind: COREWEBVIEW2_SCRIPT_DIALOG_KIND) -> DialogKind {
        match kind {
            COREWEBVIEW2_SCRIPT_DIALOG_KIND_ALERT => DialogKind::Alert,
            COREWEBVIEW2_SCRIPT_DIALOG_KIND_CONFIRM => DialogKind::Confirm,
            COREWEBVIEW2_SCRIPT_DIALOG_KIND_PROMPT => DialogKind::Prompt,
            _ => DialogKind::BeforeUnload,
        }
    }

    /// Turns WebView2's own dialogs off and handles `ScriptDialogOpening`,
    /// once per webview (WebView2's dialogs being off marks it done). Not
    /// accepting the event is the dismissed answer.
    pub(super) fn install(
        controller: &ICoreWebView2Controller,
        app_name: &str,
        post: Post,
    ) -> windows::core::Result<()> {
        // SAFETY: COM calls on the webview's own thread.
        unsafe {
            let core = controller.CoreWebView2()?;
            let settings = core.Settings()?;
            let mut enabled = BOOL::default();
            settings.AreDefaultScriptDialogsEnabled(&raw mut enabled)?;
            if !enabled.as_bool() {
                return Ok(());
            }
            let mut parent = HWND::default();
            controller.ParentWindow(&raw mut parent)?;
            let window = GetAncestor(parent, GA_ROOT);
            let caption = HSTRING::from(app_name);
            let mut token = 0_i64;
            core.add_ScriptDialogOpening(
                &ScriptDialogOpeningEventHandler::create(Box::new(move |_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let mut raw = COREWEBVIEW2_SCRIPT_DIALOG_KIND::default();
                    args.Kind(&raw mut raw)?;
                    let kind = kind_of(raw);
                    let visible = !window.is_invalid() && IsWindowVisible(window).as_bool();
                    let how = presentation(kind, crate::lab::invisible(), visible);
                    if matches!(how, Presentation::None)
                        || (matches!(how, Presentation::Blocked) && block_in_lab(kind))
                    {
                        return Ok(());
                    }
                    let mut message = PWSTR::null();
                    args.Message(&raw mut message)?;
                    let pending = Pending {
                        deferral: args.GetDeferral()?,
                        args,
                        owner: if matches!(how, Presentation::Attached) {
                            window
                        } else {
                            HWND::default()
                        },
                        message: HSTRING::from(take_pwstr(message)),
                        caption: caption.clone(),
                        kind,
                    };
                    // Shown after the handler returns: a message box runs a
                    // nested message loop, which WebView2 handlers must not.
                    post(Box::new(move || pending.run()));
                    Ok(())
                })),
                &raw mut token,
            )?;
            settings.SetAreDefaultScriptDialogsEnabled(false)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{DialogKind, Presentation, accepted, presentation};

    #[test]
    fn alert_and_confirm_show_electron_buttons() {
        assert_eq!(DialogKind::Alert.buttons(), ["OK"]);
        assert_eq!(DialogKind::Confirm.buttons(), ["OK", "Cancel"]);
    }

    #[test]
    fn prompt_and_beforeunload_show_nothing_and_are_dismissed() {
        for kind in [DialogKind::Prompt, DialogKind::BeforeUnload] {
            assert!(kind.buttons().is_empty());
            for (lab, visible) in [(false, false), (false, true), (true, true)] {
                assert_eq!(presentation(kind, lab, visible), Presentation::None);
            }
            assert!(!accepted(kind, Some(0)));
            assert!(!accepted(kind, None));
        }
    }

    #[test]
    fn a_dialog_attaches_to_a_visible_window_only() {
        assert_eq!(
            presentation(DialogKind::Alert, false, true),
            Presentation::Attached
        );
        assert_eq!(
            presentation(DialogKind::Confirm, false, false),
            Presentation::Detached
        );
    }

    #[test]
    fn the_invisible_lab_never_shows_a_dialog() {
        for kind in [DialogKind::Alert, DialogKind::Confirm] {
            for visible in [false, true] {
                assert_eq!(presentation(kind, true, visible), Presentation::Blocked);
            }
            // Blocked is answered as dismissed: confirm() is false.
            assert!(!accepted(kind, None));
        }
    }

    #[test]
    fn only_the_first_button_accepts() {
        assert!(accepted(DialogKind::Confirm, Some(0)));
        assert!(!accepted(DialogKind::Confirm, Some(1)));
        assert!(!accepted(DialogKind::Confirm, None));
        assert!(accepted(DialogKind::Alert, Some(0)));
    }

    #[test]
    fn lab_names() {
        assert_eq!(DialogKind::Alert.name(), "alert");
        assert_eq!(DialogKind::Confirm.name(), "confirm");
        assert_eq!(DialogKind::Prompt.name(), "prompt");
        assert_eq!(DialogKind::BeforeUnload.name(), "beforeunload");
    }
}
