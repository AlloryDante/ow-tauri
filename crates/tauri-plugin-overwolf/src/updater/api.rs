//! The update client's Rust API (DESIGN §3.3, D18; Windows (R6)), shaped
//! like `tauri-plugin-updater`'s. The signatures are final; the engine is
//! wired in W3, until then [`Updater::check`] answers `unsupported`.

use std::time::Duration;

use serde_json::Value;
use tauri::{AppHandle, Runtime};
use url::Url;

use crate::error::{Error, Result};

fn unavailable() -> Error {
    Error::unsupported("the update client is not available in this build yet")
}

/// Configures an [`Updater`]. The feed URL is configuration only
/// (`updater.endpoint`): code can never point the updater at another feed.
///
/// ```no_run
/// use tauri_plugin_overwolf::OverwolfExt;
/// # fn example(app: &tauri::AppHandle) -> tauri_plugin_overwolf::Result<()> {
/// let updater = app
///     .overwolf()
///     .updater_builder()
///     .allow_prerelease(true)
///     .connect_timeout(std::time::Duration::from_secs(10))
///     .build()?;
/// # let _ = updater;
/// # Ok(())
/// # }
/// ```
#[must_use]
pub struct UpdaterBuilder<R: Runtime> {
    app: AppHandle<R>,
    channel: Option<String>,
    allow_downgrade: Option<bool>,
    allow_prerelease: Option<bool>,
    headers: Vec<(String, String)>,
    connect_timeout: Option<Duration>,
    read_timeout: Option<Duration>,
}

impl<R: Runtime> std::fmt::Debug for UpdaterBuilder<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpdaterBuilder")
            .field("channel", &self.channel)
            .field("allow_downgrade", &self.allow_downgrade)
            .field("allow_prerelease", &self.allow_prerelease)
            .field("headers", &self.headers.len())
            .finish_non_exhaustive()
    }
}

impl<R: Runtime> UpdaterBuilder<R> {
    /// A builder with the configured defaults.
    pub(crate) fn new(app: AppHandle<R>) -> Self {
        UpdaterBuilder {
            app,
            channel: None,
            allow_downgrade: None,
            allow_prerelease: None,
            headers: Vec::new(),
            connect_timeout: None,
            read_timeout: None,
        }
    }

    /// The channel (`latest`, `beta`, ...). Also allows a downgrade, as
    /// electron-updater does when a channel is set (CONTRACT I.1).
    pub fn channel(mut self, channel: impl Into<String>) -> Self {
        self.channel = Some(channel.into());
        self.allow_downgrade = Some(true);
        self
    }

    /// Whether a lower version may be offered.
    pub fn allow_downgrade(mut self, allow: bool) -> Self {
        self.allow_downgrade = Some(allow);
        self
    }

    /// Whether pre-release versions may be offered.
    pub fn allow_prerelease(mut self, allow: bool) -> Self {
        self.allow_prerelease = Some(allow);
        self
    }

    /// A request header for the feed host only: dropped on cross-origin
    /// redirects and on the download host (SEC-M10).
    ///
    /// # Errors
    ///
    /// `invalid-argument` for a header name or value HTTP refuses.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Result<Self> {
        let (name, value) = (name.into(), value.into());
        let name_ok = !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b));
        let value_ok = value
            .bytes()
            .all(|b| b == b'\t' || (0x20..0x7f).contains(&b));
        if !name_ok || !value_ok {
            return Err(Error::invalid_argument("invalid update request header"));
        }
        self.headers.push((name, value));
        Ok(self)
    }

    /// The connection timeout.
    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = Some(timeout);
        self
    }

    /// The idle read timeout (not a total timeout).
    pub fn read_timeout(mut self, timeout: Duration) -> Self {
        self.read_timeout = Some(timeout);
        self
    }

    /// The updater.
    ///
    /// # Errors
    ///
    /// `config` without `updater.endpoint`.
    pub fn build(self) -> Result<Updater<R>> {
        Ok(Updater { builder: self })
    }
}

/// Checks the configured feed for an update.
pub struct Updater<R: Runtime> {
    builder: UpdaterBuilder<R>,
}

impl<R: Runtime> std::fmt::Debug for Updater<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Updater")
            .field("builder", &self.builder)
            .finish()
    }
}

impl<R: Runtime> Updater<R> {
    /// Reads the feed; `None` when no update is offered.
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # async fn example(app: tauri::AppHandle) -> tauri_plugin_overwolf::Result<()> {
    /// if let Some(update) = app.overwolf().updater()?.check().await? {
    ///     update.download_and_install(|_, _| {}, || {}).await?;
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// `network` for a failed request, `verification` for a bad feed,
    /// `unsupported` until the engine is wired.
    #[allow(
        unknown_lints,
        clippy::unused_async,
        clippy::unused_async_trait_impl,
        reason = "frozen async interface; the update engine (W3) awaits"
    )]
    pub async fn check(&self) -> Result<Option<Update>> {
        let _ = &self.builder.app;
        Err(unavailable())
    }
}

/// An available update.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Update {
    /// The new version.
    pub version: String,
    /// The running version.
    pub current_version: String,
    /// The release date.
    pub date: Option<String>,
    /// The release notes.
    pub body: Option<String>,
    /// The feed entry as received.
    pub raw: Value,
    /// The installer's URL.
    pub download_url: Url,
    /// Whether this client passed the `stagingPercentage` gate.
    pub staged: bool,
}

/// A downloaded, verified installer.
#[derive(Debug)]
pub struct DownloadedUpdate {
    _private: (),
}

impl Update {
    /// Downloads and verifies the installer; `on_chunk(length, total)` per
    /// chunk, `on_finish()` at the end.
    ///
    /// # Errors
    ///
    /// `network`, `io` or `verification`.
    #[allow(
        unknown_lints,
        clippy::unused_async,
        clippy::unused_async_trait_impl,
        reason = "frozen async interface; the update engine (W3) awaits"
    )]
    pub async fn download<C: FnMut(usize, Option<u64>), D: FnOnce()>(
        &self,
        on_chunk: C,
        on_finish: D,
    ) -> Result<DownloadedUpdate> {
        drop((on_chunk, on_finish));
        Err(unavailable())
    }

    /// Re-verifies the installer (hash and signer), starts it with
    /// `/UPDATE` and exits the app (an explicit app request).
    ///
    /// # Errors
    ///
    /// `verification` or `io`.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "frozen interface: installing consumes the download (W3)"
    )]
    pub fn install(&self, downloaded: DownloadedUpdate) -> Result<()> {
        let DownloadedUpdate { _private: () } = downloaded;
        Err(unavailable())
    }

    /// [`Update::download`] then [`Update::install`].
    ///
    /// # Errors
    ///
    /// As both.
    pub async fn download_and_install<C: FnMut(usize, Option<u64>), D: FnOnce()>(
        &self,
        on_chunk: C,
        on_finish: D,
    ) -> Result<()> {
        let downloaded = self.download(on_chunk, on_finish).await?;
        self.install(downloaded)
    }
}
