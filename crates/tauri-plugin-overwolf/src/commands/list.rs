// The commands this plugin registers (DESIGN §3.5). Shared by the build
// script (which generates one `allow-<command>` permission per entry) and the
// crate. Keep in sync with `permissions/*.toml` and the invoke handler in
// `commands/mod.rs`; a unit test checks both.

/// Every command the plugin registers, in wire spelling (25).
pub const COMMANDS: &[&str] = &[
    // `<owadview>` (app webviews; `overwolf:default`)
    "adview_mount",
    "adview_update",
    "adview_unmount",
    "adview_command",
    // windows and identity (`overwolf:default`)
    "set_window_name",
    "get_info",
    // `overwolf:machine-id`
    "get_machine_ids",
    // consent (`overwolf:default`)
    "is_cmp_required",
    "open_ad_privacy_settings_window",
    "open_cmp_window",
    // switches that reduce what is sent (`overwolf:default`)
    "disable_anonymous_analytics",
    "disable_ads_optimization",
    "disable_ads_fpd",
    // `overwolf:email-hashes`
    "generate_user_email_hashes",
    "set_user_email_hashes",
    "clear_user_email_hashes",
    // `overwolf:analytics`
    "set_external_payment_user_id",
    "set_analytics_user_enabled",
    "set_anonymous_analytics_preference",
    // `overwolf:updater` (Windows with the `updater` feature)
    "updater_check",
    "updater_download",
    "updater_install",
    "updater_download_and_install",
    // runtime capabilities only: ad guests (`overwolf:adview-guest`) and the
    // consent windows (`overwolf:cmp-window`)
    "adview_event",
    "cmp_event",
];
