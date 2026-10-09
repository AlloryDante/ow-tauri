//! The update client's Rust API (DESIGN §3.3, D18; Windows (R6)), shaped
//! like `tauri-plugin-updater`'s.

use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tauri::{AppHandle, Runtime};
use url::Url;

use super::engine::{self, Ctx, Found, Overrides, Slot, Target};
use crate::error::{Error, Result};

/// Configures an [`Updater`]. The feed URL is configuration only
/// (`updater.endpoint`): code can never point the updater at another feed.
///
/// ```no_run
/// # #[cfg(windows)]
/// # fn example(app: &tauri::AppHandle) -> tauri_plugin_overwolf::Result<()> {
/// use tauri_plugin_overwolf::OverwolfExt;
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
    overrides: Overrides,
}

impl<R: Runtime> std::fmt::Debug for UpdaterBuilder<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpdaterBuilder")
            .field("channel", &self.overrides.channel)
            .field("allow_downgrade", &self.overrides.allow_downgrade)
            .field("allow_prerelease", &self.overrides.allow_prerelease)
            .field("headers", &self.overrides.headers.len())
            .finish_non_exhaustive()
    }
}

impl<R: Runtime> UpdaterBuilder<R> {
    /// A builder with the configured defaults.
    pub(crate) fn new(app: AppHandle<R>) -> Self {
        UpdaterBuilder {
            app,
            overrides: Overrides::default(),
        }
    }

    /// The channel (`latest`, `beta`, ...). Also allows a downgrade, as
    /// electron-updater does when a channel is set (CONTRACT I.1); call
    /// [`allow_downgrade`](Self::allow_downgrade) after it to turn that
    /// off.
    pub fn channel(mut self, channel: impl Into<String>) -> Self {
        self.overrides.channel = Some(channel.into());
        self.overrides.allow_downgrade = Some(true);
        self
    }

    /// Whether a lower version may be offered.
    pub fn allow_downgrade(mut self, allow: bool) -> Self {
        self.overrides.allow_downgrade = Some(allow);
        self
    }

    /// Whether pre-release versions may be offered.
    pub fn allow_prerelease(mut self, allow: bool) -> Self {
        self.overrides.allow_prerelease = Some(allow);
        self
    }

    /// A request header for the feed origin only: dropped on cross-origin
    /// redirects and never sent to the download host (SEC-M10).
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
        self.overrides.headers.push((name, value));
        Ok(self)
    }

    /// The connection timeout.
    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.overrides.connect_timeout = Some(timeout);
        self
    }

    /// The idle read timeout: the longest wait for the next bytes, not a
    /// limit on the whole download.
    pub fn read_timeout(mut self, timeout: Duration) -> Self {
        self.overrides.read_timeout = Some(timeout);
        self
    }

    /// The updater.
    ///
    /// # Errors
    ///
    /// `unsupported` off Windows (R6); `config` when neither
    /// `updater.publisherNames` nor `updater.pubkey` is set (R5; a debug
    /// build may set `dangerousSkipPublisherCheck`); `invalid-argument` for
    /// a bad channel, feed URL or key.
    pub fn build(self) -> Result<Updater<R>> {
        let ctx = Ctx::new(&self.app, &self.overrides)?;
        Ok(Updater {
            ctx: Arc::new(ctx),
            runtime: PhantomData,
        })
    }
}

/// Checks the configured feed for an update.
pub struct Updater<R: Runtime> {
    ctx: Arc<Ctx>,
    runtime: PhantomData<fn() -> R>,
}

impl<R: Runtime> std::fmt::Debug for Updater<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Updater").field("ctx", &self.ctx).finish()
    }
}

impl<R: Runtime> Updater<R> {
    /// Reads the feed; `None` when no update is offered (same or older
    /// version, a prerelease with prereleases off, an OS below
    /// `minimumSystemVersion`, or outside the staged rollout).
    ///
    /// ```no_run
    /// # #[cfg(windows)]
    /// # async fn example(app: tauri::AppHandle) -> tauri_plugin_overwolf::Result<()> {
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// if let Some(update) = app.overwolf().updater()?.check().await? {
    ///     update.download_and_install(|_, _| {}, || {}).await?;
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// `network` for a failed request, `invalid-argument` for a bad feed,
    /// `unsupported` for a release that offers only an `.msi`.
    pub async fn check(&self) -> Result<Option<Update>> {
        let found = engine::check(&self.ctx).await?;
        Ok(found.map(|f| Update::new(&self.ctx, f)))
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
    /// Whether the feed had a `stagingPercentage` and this install passed
    /// it.
    pub staged: bool,
    ctx: Arc<Ctx>,
    target: Target,
}

/// A downloaded, verified installer, held open until it is installed.
#[derive(Debug)]
pub struct DownloadedUpdate {
    slot: Arc<Slot>,
}

/// The release notes as one text: a string, or the `note`s of a list of
/// `{ version, note }`.
fn notes(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(s) => Some(s.clone()),
        Value::Array(items) => {
            let parts: Vec<&str> = items
                .iter()
                .filter_map(|i| i.get("note").and_then(Value::as_str))
                .collect();
            (!parts.is_empty()).then(|| parts.join("\n\n"))
        }
        _ => None,
    }
}

impl Update {
    fn new(ctx: &Arc<Ctx>, found: Found) -> Self {
        let Found {
            info,
            target,
            staged,
        } = found;
        Update {
            version: info.version.clone(),
            current_version: ctx.current_version().to_owned(),
            date: Some(info.release_date.clone()).filter(|d| !d.is_empty()),
            body: notes(info.release_notes.as_ref()),
            raw: serde_json::to_value(&info).unwrap_or(Value::Null),
            download_url: target.url.clone(),
            staged,
            ctx: Arc::clone(ctx),
            target,
        }
    }

    /// Downloads and verifies the installer; `on_chunk(length, total)` per
    /// chunk, `on_finish()` once the bytes are in. With
    /// `updater.installOnExit` (the default) the verified installer also
    /// starts silently when the app exits, unless [`Update::install`] ran.
    /// A newer download replaces an earlier one.
    ///
    /// # Errors
    ///
    /// `network`, `io` or `verification`.
    pub async fn download<C: FnMut(usize, Option<u64>), D: FnOnce()>(
        &self,
        on_chunk: C,
        on_finish: D,
    ) -> Result<DownloadedUpdate> {
        let slot = engine::download(&self.ctx, &self.target, on_chunk, on_finish).await?;
        Ok(DownloadedUpdate { slot })
    }

    /// Re-verifies the installer (hash and signer), starts it with
    /// `/UPDATE /R` (or `updater.installerArgs`, always with `/UPDATE`)
    /// and exits the app (an explicit app request).
    ///
    /// # Errors
    ///
    /// `verification`, `io`, or `not-found` when the download was already
    /// installed or replaced.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "frozen interface: installing consumes the download"
    )]
    pub fn install(&self, downloaded: DownloadedUpdate) -> Result<()> {
        engine::install(&self.ctx, &downloaded.slot, &self.version)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_notes_text() {
        assert_eq!(notes(None), None);
        assert_eq!(
            notes(Some(&serde_json::json!("fixes"))).as_deref(),
            Some("fixes")
        );
        let list = serde_json::json!([{ "version": "2", "note": "a" }, { "version": "1", "note": "b" }]);
        assert_eq!(notes(Some(&list)).as_deref(), Some("a\n\nb"));
        assert_eq!(notes(Some(&serde_json::json!([1]))), None);
    }
}
