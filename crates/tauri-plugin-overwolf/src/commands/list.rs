// The commands this plugin registers. Shared by the build script (which
// generates one `allow-<command>` permission per entry) and the crate.
// Keep in sync with `permissions/*.toml` and the invoke handler.

/// Every command the plugin registers, in wire spelling.
pub const COMMANDS: &[&str] = &[
    // A.2.1 bootstrap and lifecycle
    "ipc_subscribe",
    "bootstrap",
    "main_ready",
    "ipc_main_ready",
    "app_quit_reply",
    "app_relaunch",
    "app_quit",
    "app_exit",
    "app_focus",
    "log",
    "ipc_reply",
    "ipc_emit",
    "ipc_emit_skip",
    // A.2.2 session switches
    "disable_anonymous_analytics",
    "disable_ads_optimization",
    "disable_ads_fpd",
    "is_cmp_required",
    "open_cmp_window",
    "open_ad_privacy_settings_window",
    "set_user_email_hashes",
    "set_external_payment_user_id",
    "analytics_set_user_enabled",
    // A.1.1 browser switches recorded after main_ready
    "app_record_browser_args",
    // A.2.4 packages
    "packages_snapshot",
    "packages_relaunch",
    "packages_set_channel",
    "packages_get_available_channels",
    "packages_get_channel",
    // A.2.3 windows, screen, shell, dialogs, files
    "window_create",
    "window_load",
    "window_close_reply",
    "window_destroy",
    "window_eval",
    "window_devtools",
    "window_set_name",
    "screen_snapshot",
    "shell_open_external",
    "shell_open_path",
    "shell_show_item_in_folder",
    "dialog_open",
    "dialog_save",
    "dialog_message",
    "global_shortcut_register",
    "global_shortcut_unregister",
    "fs_read_text",
    "fs_write_text",
    "fs_exists",
    "fs_mkdir",
    // A.2.8 updater
    "updater_configure",
    "updater_check",
    "updater_download",
    "updater_quit_and_install",
    // A.2.5 UI windows
    "ipc_invoke",
    "ipc_send",
    "ipc_skip",
    "eval_result",
    "navigation_external",
    "adview_mount",
    "adview_update",
    "adview_unmount",
    "adview_command",
    // A.2.6 ad guests
    "adview_event",
    // A.2.7 consent windows
    "cmp_event",
];
