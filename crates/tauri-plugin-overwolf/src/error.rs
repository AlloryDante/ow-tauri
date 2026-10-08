//! Typed errors (DESIGN §3.3).
//!
//! Every command returns [`Error`] on failure. It serialises as the wire shape
//! the JavaScript API (`tauri-plugin-overwolf-api`) turns into an
//! `OverwolfError` with the same `code`:
//!
//! ```json
//! { "code": "network", "message": "network: the feed answered 503", "data": { "status": 503 } }
//! ```
//!
//! The codes are `ERROR_CODES` of `packages/api/src/errors.ts`; a unit test
//! pins the list.

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

use crate::config::ConfigError;

/// The result type of the plugin's API.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Every failure of the plugin's API.
///
/// ```
/// use tauri_plugin_overwolf::{Error, ErrorCode};
/// let err = Error::unsupported("ads are not available on Linux");
/// assert_eq!(err.code(), ErrorCode::Unsupported);
/// assert_eq!(err.to_string(), "unsupported: ads are not available on Linux");
/// ```
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Not available on this platform, build or configuration.
    #[error("unsupported: {message}")]
    Unsupported {
        /// What is not available.
        message: String,
    },
    /// An argument failed validation.
    #[error("invalid argument: {message}")]
    InvalidArgument {
        /// What is wrong.
        message: String,
    },
    /// The window, element or update does not exist.
    #[error("not found: {message}")]
    NotFound {
        /// What is missing.
        message: String,
    },
    /// The caller may not make this call.
    #[error("forbidden: {message}")]
    Forbidden {
        /// Why.
        message: String,
    },
    /// A file-system or window-system operation failed.
    #[error("i/o: {message}")]
    Io {
        /// What failed (never a path).
        message: String,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// An HTTP request failed.
    #[error("network: {message}")]
    Network {
        /// What failed.
        message: String,
        /// The HTTP status, when a response arrived.
        status: Option<u16>,
    },
    /// An update failed its signature or hash check.
    #[error("verification failed: {message}")]
    Verification {
        /// What failed.
        message: String,
    },
    /// Another failure inside the plugin.
    #[error("backend: {message}")]
    Backend {
        /// What failed.
        message: String,
    },
    /// The plugin configuration is invalid.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// Tauri reported an error.
    #[cfg(feature = "plugin")]
    #[error(transparent)]
    Tauri(#[from] tauri::Error),
}

/// Machine-readable error code, one per [`Error`] variant.
///
/// ```
/// use tauri_plugin_overwolf::ErrorCode;
/// assert_eq!(ErrorCode::InvalidArgument.as_str(), "invalid-argument");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorCode {
    /// [`Error::Unsupported`].
    Unsupported,
    /// [`Error::InvalidArgument`].
    InvalidArgument,
    /// [`Error::NotFound`].
    NotFound,
    /// [`Error::Forbidden`].
    Forbidden,
    /// [`Error::Io`].
    Io,
    /// [`Error::Network`].
    Network,
    /// [`Error::Verification`].
    Verification,
    /// [`Error::Backend`].
    Backend,
    /// [`Error::Config`].
    Config,
    /// `Error::Tauri` (feature `plugin`).
    Tauri,
}

impl ErrorCode {
    /// Every code, in the order of the JavaScript `ERROR_CODES`.
    pub const ALL: [ErrorCode; 10] = [
        ErrorCode::Unsupported,
        ErrorCode::InvalidArgument,
        ErrorCode::NotFound,
        ErrorCode::Forbidden,
        ErrorCode::Io,
        ErrorCode::Network,
        ErrorCode::Verification,
        ErrorCode::Backend,
        ErrorCode::Config,
        ErrorCode::Tauri,
    ];

    /// The wire string of this code.
    ///
    /// ```
    /// use tauri_plugin_overwolf::ErrorCode;
    /// assert_eq!(ErrorCode::NotFound.as_str(), "not-found");
    /// ```
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ErrorCode::Unsupported => "unsupported",
            ErrorCode::InvalidArgument => "invalid-argument",
            ErrorCode::NotFound => "not-found",
            ErrorCode::Forbidden => "forbidden",
            ErrorCode::Io => "io",
            ErrorCode::Network => "network",
            ErrorCode::Verification => "verification",
            ErrorCode::Backend => "backend",
            ErrorCode::Config => "config",
            ErrorCode::Tauri => "tauri",
        }
    }
}

impl Error {
    /// [`Error::Unsupported`] with `message`.
    #[must_use]
    pub fn unsupported(message: impl Into<String>) -> Self {
        Error::Unsupported {
            message: message.into(),
        }
    }

    /// [`Error::InvalidArgument`] with `message`.
    #[must_use]
    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Error::InvalidArgument {
            message: message.into(),
        }
    }

    /// [`Error::NotFound`] with `message`.
    #[must_use]
    pub fn not_found(message: impl Into<String>) -> Self {
        Error::NotFound {
            message: message.into(),
        }
    }

    /// [`Error::Forbidden`] with `message`.
    #[must_use]
    pub fn forbidden(message: impl Into<String>) -> Self {
        Error::Forbidden {
            message: message.into(),
        }
    }

    /// [`Error::Backend`] with `message`.
    #[must_use]
    pub fn backend(message: impl Into<String>) -> Self {
        Error::Backend {
            message: message.into(),
        }
    }

    /// [`Error::Verification`] with `message`.
    #[must_use]
    pub fn verification(message: impl Into<String>) -> Self {
        Error::Verification {
            message: message.into(),
        }
    }

    /// [`Error::Network`] with `message` and the HTTP `status`, if any.
    #[must_use]
    pub fn network(message: impl Into<String>, status: Option<u16>) -> Self {
        Error::Network {
            message: message.into(),
            status,
        }
    }

    /// [`Error::Io`] for `source` while `context`. The message names the
    /// operation and the error kind only, never a path (paths can carry the
    /// user name).
    ///
    /// ```
    /// use tauri_plugin_overwolf::Error;
    /// let io = std::io::Error::new(std::io::ErrorKind::NotFound, "/home/someone/x missing");
    /// let err = Error::from_io("Reading the state file", &io);
    /// assert!(!err.to_string().contains("someone"));
    /// ```
    #[must_use]
    pub fn from_io(context: &str, source: &std::io::Error) -> Self {
        let kind = source.kind();
        Error::Io {
            message: format!("{context} failed ({kind})."),
            source: std::io::Error::new(kind, kind.to_string()),
        }
    }

    /// The code of this error.
    #[must_use]
    pub fn code(&self) -> ErrorCode {
        match self {
            Error::Unsupported { .. } => ErrorCode::Unsupported,
            Error::InvalidArgument { .. } => ErrorCode::InvalidArgument,
            Error::NotFound { .. } => ErrorCode::NotFound,
            Error::Forbidden { .. } => ErrorCode::Forbidden,
            Error::Io { .. } => ErrorCode::Io,
            Error::Network { .. } => ErrorCode::Network,
            Error::Verification { .. } => ErrorCode::Verification,
            Error::Backend { .. } => ErrorCode::Backend,
            Error::Config(_) => ErrorCode::Config,
            #[cfg(feature = "plugin")]
            Error::Tauri(_) => ErrorCode::Tauri,
        }
    }
}

impl Serialize for Error {
    /// `{ "code": <wire>, "message": <Display>, "data"?: { "status": u16 } }`.
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("OverwolfErrorWire", 3)?;
        s.serialize_field("code", self.code().as_str())?;
        s.serialize_field("message", &self.to_string())?;
        match self {
            Error::Network {
                status: Some(status),
                ..
            } => s.serialize_field("data", &serde_json::json!({ "status": status }))?,
            _ => s.skip_field("data")?,
        }
        s.end()
    }
}

#[cfg(test)]
mod tests {
    use super::{Error, ErrorCode};

    /// `ERROR_CODES` of `packages/api/src/errors.ts`, in order.
    const TS_CODES: [&str; 10] = [
        "unsupported",
        "invalid-argument",
        "not-found",
        "forbidden",
        "io",
        "network",
        "verification",
        "backend",
        "config",
        "tauri",
    ];

    #[test]
    fn codes_match_the_typescript_list() {
        let rust: Vec<&str> = ErrorCode::ALL.iter().map(|c| c.as_str()).collect();
        assert_eq!(rust, TS_CODES);
    }

    /// Regression (W1 gate): the pin read a deleted file and skipped
    /// silently. In the repository the file must exist and list every code,
    /// in order; only a crate built outside the repository (crates.io,
    /// docs.rs: no `packages/` next to it) skips.
    #[test]
    fn typescript_source_lists_every_code() {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let repo = manifest.join("../../packages");
        if !repo.is_dir() {
            return;
        }
        let path = repo.join("api/src/errors.ts");
        let source = std::fs::read_to_string(&path).expect("packages/api/src/errors.ts exists");
        let list = source
            .split("export const ERROR_CODES")
            .nth(1)
            .and_then(|rest| rest.split("]);").next())
            .expect("errors.ts declares ERROR_CODES");
        let listed: Vec<&str> = list.split('\'').skip(1).step_by(2).collect();
        assert_eq!(listed, TS_CODES);
    }

    #[test]
    fn wire_shape() {
        let err = Error::network("the feed answered 503", Some(503));
        let wire = serde_json::to_value(&err).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({
                "code": "network",
                "message": "network: the feed answered 503",
                "data": { "status": 503 }
            })
        );
        let wire = serde_json::to_value(Error::forbidden("no")).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({ "code": "forbidden", "message": "forbidden: no" })
        );
    }

    #[test]
    fn every_constructor_has_its_code() {
        let io = std::io::Error::other("x");
        let cases = [
            (Error::unsupported("m"), ErrorCode::Unsupported),
            (Error::invalid_argument("m"), ErrorCode::InvalidArgument),
            (Error::not_found("m"), ErrorCode::NotFound),
            (Error::forbidden("m"), ErrorCode::Forbidden),
            (Error::from_io("m", &io), ErrorCode::Io),
            (Error::network("m", None), ErrorCode::Network),
            (Error::verification("m"), ErrorCode::Verification),
            (Error::backend("m"), ErrorCode::Backend),
            (
                Error::from(crate::config::ConfigError::new("uid", "bad")),
                ErrorCode::Config,
            ),
        ];
        for (err, code) in cases {
            assert_eq!(err.code(), code);
            let wire = serde_json::to_value(&err).unwrap();
            assert_eq!(wire["code"], code.as_str());
        }
    }

    #[test]
    #[cfg(feature = "plugin")]
    fn tauri_errors_map_to_tauri() {
        let err = Error::from(tauri::Error::WebviewNotFound);
        assert_eq!(err.code(), ErrorCode::Tauri);
    }

    #[test]
    fn config_errors_name_the_path() {
        let err = Error::from(crate::config::ConfigError::new(
            "uid",
            "must be 1 to 64 ASCII letters or digits",
        ));
        assert_eq!(
            err.to_string(),
            "plugins.overwolf.uid: must be 1 to 64 ASCII letters or digits"
        );
    }
}
