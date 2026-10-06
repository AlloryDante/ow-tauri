//! Typed errors (CONTRACT section A.4).
//!
//! Every command returns [`Error`] on failure. It serialises as the
//! `OverwolfErrorWire` shape the JavaScript runtime turns into an
//! `OwTauriError` with the same `code`:
//!
//! ```json
//! { "code": "not-found", "message": "No window with id 7.", "data": { "id": 7 } }
//! ```
//!
//! The codes are the strings of `OwTauriErrorCode` in
//! `packages/ow-tauri/src/shared/errors.ts`; a unit test pins the list.

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};
use serde_json::Value;

/// Machine-readable error code, one per [`Error`] variant.
///
/// The string form ([`ErrorCode::as_str`]) is the `code` field on the wire.
///
/// ```
/// use tauri_plugin_overwolf::ErrorCode;
/// assert_eq!(ErrorCode::IpcOverloaded.as_str(), "ipc-overloaded");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    /// The API exists in Electron or ow-electron but ow-tauri does not implement it.
    Unsupported,
    /// The host (or a package) is not ready for the call.
    NotReady,
    /// An argument failed validation.
    InvalidArgument,
    /// The window, element, event or request does not exist.
    NotFound,
    /// The caller's window class may not make the call, or a path is out of scope.
    Forbidden,
    /// No `ipcMain.handle` handler for the channel.
    IpcNoHandler,
    /// An IPC request got no reply in time.
    IpcTimeout,
    /// A value cannot be encoded for IPC, or the message is too large.
    IpcSerialization,
    /// The `ipcMain.handle` handler threw or rejected.
    IpcRemoteError,
    /// Too many invokes in flight or messages queued.
    IpcOverloaded,
    /// A file-system or window-system operation failed.
    Io,
    /// An HTTP request failed.
    Network,
    /// The plugin or a package runtime reported another failure.
    Backend,
}

impl ErrorCode {
    /// Every code, in the order of the contract's list.
    pub const ALL: [ErrorCode; 13] = [
        ErrorCode::Unsupported,
        ErrorCode::NotReady,
        ErrorCode::InvalidArgument,
        ErrorCode::NotFound,
        ErrorCode::Forbidden,
        ErrorCode::IpcNoHandler,
        ErrorCode::IpcTimeout,
        ErrorCode::IpcSerialization,
        ErrorCode::IpcRemoteError,
        ErrorCode::IpcOverloaded,
        ErrorCode::Io,
        ErrorCode::Network,
        ErrorCode::Backend,
    ];

    /// The wire string of this code.
    ///
    /// ```
    /// use tauri_plugin_overwolf::ErrorCode;
    /// assert_eq!(ErrorCode::NotReady.as_str(), "not-ready");
    /// ```
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ErrorCode::Unsupported => "unsupported",
            ErrorCode::NotReady => "not-ready",
            ErrorCode::InvalidArgument => "invalid-argument",
            ErrorCode::NotFound => "not-found",
            ErrorCode::Forbidden => "forbidden",
            ErrorCode::IpcNoHandler => "ipc-no-handler",
            ErrorCode::IpcTimeout => "ipc-timeout",
            ErrorCode::IpcSerialization => "ipc-serialization",
            ErrorCode::IpcRemoteError => "ipc-remote-error",
            ErrorCode::IpcOverloaded => "ipc-overloaded",
            ErrorCode::Io => "io",
            ErrorCode::Network => "network",
            ErrorCode::Backend => "backend",
        }
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The error type of every plugin command and of the Rust API.
///
/// One variant per [`ErrorCode`]. Each carries an English, single-sentence
/// message without secrets or user data, and optional code-specific `data`.
///
/// ```
/// use tauri_plugin_overwolf::Error;
/// let err = Error::not_found("No window with id 7.");
/// let wire = serde_json::to_value(&err).unwrap();
/// assert_eq!(wire["code"], "not-found");
/// assert_eq!(wire["message"], "No window with id 7.");
/// assert!(wire.get("data").is_none());
/// ```
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum Error {
    /// See [`ErrorCode::Unsupported`].
    #[error("{message}")]
    Unsupported {
        /// Human-readable message.
        message: String,
        /// Code-specific details.
        data: Option<Value>,
    },
    /// See [`ErrorCode::NotReady`].
    #[error("{message}")]
    NotReady {
        /// Human-readable message.
        message: String,
        /// Code-specific details.
        data: Option<Value>,
    },
    /// See [`ErrorCode::InvalidArgument`].
    #[error("{message}")]
    InvalidArgument {
        /// Human-readable message.
        message: String,
        /// Code-specific details.
        data: Option<Value>,
    },
    /// See [`ErrorCode::NotFound`].
    #[error("{message}")]
    NotFound {
        /// Human-readable message.
        message: String,
        /// Code-specific details.
        data: Option<Value>,
    },
    /// See [`ErrorCode::Forbidden`].
    #[error("{message}")]
    Forbidden {
        /// Human-readable message.
        message: String,
        /// Code-specific details.
        data: Option<Value>,
    },
    /// See [`ErrorCode::IpcNoHandler`].
    #[error("{message}")]
    IpcNoHandler {
        /// Human-readable message.
        message: String,
        /// Code-specific details.
        data: Option<Value>,
    },
    /// See [`ErrorCode::IpcTimeout`].
    #[error("{message}")]
    IpcTimeout {
        /// Human-readable message.
        message: String,
        /// Code-specific details.
        data: Option<Value>,
    },
    /// See [`ErrorCode::IpcSerialization`].
    #[error("{message}")]
    IpcSerialization {
        /// Human-readable message.
        message: String,
        /// Code-specific details.
        data: Option<Value>,
    },
    /// See [`ErrorCode::IpcRemoteError`].
    #[error("{message}")]
    IpcRemoteError {
        /// Human-readable message.
        message: String,
        /// Code-specific details.
        data: Option<Value>,
    },
    /// See [`ErrorCode::IpcOverloaded`].
    #[error("{message}")]
    IpcOverloaded {
        /// Human-readable message.
        message: String,
        /// Code-specific details.
        data: Option<Value>,
    },
    /// See [`ErrorCode::Io`].
    #[error("{message}")]
    Io {
        /// Human-readable message.
        message: String,
        /// Code-specific details.
        data: Option<Value>,
    },
    /// See [`ErrorCode::Network`].
    #[error("{message}")]
    Network {
        /// Human-readable message.
        message: String,
        /// Code-specific details.
        data: Option<Value>,
    },
    /// See [`ErrorCode::Backend`].
    #[error("{message}")]
    Backend {
        /// Human-readable message.
        message: String,
        /// Code-specific details.
        data: Option<Value>,
    },
}

/// Shorthand for `Result<T, tauri_plugin_overwolf::Error>`.
pub type Result<T, E = Error> = std::result::Result<T, E>;

macro_rules! constructors {
    ($($(#[$doc:meta])* $fn_name:ident => $variant:ident;)*) => {
        $(
            $(#[$doc])*
            #[must_use]
            pub fn $fn_name(message: impl Into<String>) -> Self {
                Error::$variant { message: message.into(), data: None }
            }
        )*
    };
}

impl Error {
    /// Builds an error from a code, a message and optional data.
    ///
    /// ```
    /// use tauri_plugin_overwolf::{Error, ErrorCode};
    /// let err = Error::new(ErrorCode::Io, "Disk full.", None);
    /// assert_eq!(err.code(), ErrorCode::Io);
    /// ```
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>, data: Option<Value>) -> Self {
        let message = message.into();
        match code {
            ErrorCode::Unsupported => Error::Unsupported { message, data },
            ErrorCode::NotReady => Error::NotReady { message, data },
            ErrorCode::InvalidArgument => Error::InvalidArgument { message, data },
            ErrorCode::NotFound => Error::NotFound { message, data },
            ErrorCode::Forbidden => Error::Forbidden { message, data },
            ErrorCode::IpcNoHandler => Error::IpcNoHandler { message, data },
            ErrorCode::IpcTimeout => Error::IpcTimeout { message, data },
            ErrorCode::IpcSerialization => Error::IpcSerialization { message, data },
            ErrorCode::IpcRemoteError => Error::IpcRemoteError { message, data },
            ErrorCode::IpcOverloaded => Error::IpcOverloaded { message, data },
            ErrorCode::Io => Error::Io { message, data },
            ErrorCode::Network => Error::Network { message, data },
            ErrorCode::Backend => Error::Backend { message, data },
        }
    }

    constructors! {
        /// An [`ErrorCode::Unsupported`] error.
        unsupported => Unsupported;
        /// An [`ErrorCode::NotReady`] error.
        not_ready => NotReady;
        /// An [`ErrorCode::InvalidArgument`] error.
        invalid_argument => InvalidArgument;
        /// An [`ErrorCode::NotFound`] error.
        not_found => NotFound;
        /// An [`ErrorCode::Forbidden`] error.
        forbidden => Forbidden;
        /// An [`ErrorCode::IpcSerialization`] error.
        ipc_serialization => IpcSerialization;
        /// An [`ErrorCode::IpcOverloaded`] error.
        ipc_overloaded => IpcOverloaded;
        /// An [`ErrorCode::IpcTimeout`] error.
        ipc_timeout => IpcTimeout;
        /// An [`ErrorCode::Io`] error.
        io => Io;
        /// An [`ErrorCode::Network`] error.
        network => Network;
        /// An [`ErrorCode::Backend`] error.
        backend => Backend;
    }

    /// Returns the same error with `data` attached.
    ///
    /// ```
    /// use tauri_plugin_overwolf::Error;
    /// let err = Error::not_found("No window with that id.").with_data(serde_json::json!({ "id": 7 }));
    /// assert_eq!(serde_json::to_value(&err).unwrap()["data"]["id"], 7);
    /// ```
    #[must_use]
    pub fn with_data(self, data: Value) -> Self {
        let code = self.code();
        Error::new(code, self.message().to_owned(), Some(data))
    }

    /// The machine-readable code.
    #[must_use]
    pub fn code(&self) -> ErrorCode {
        match self {
            Error::Unsupported { .. } => ErrorCode::Unsupported,
            Error::NotReady { .. } => ErrorCode::NotReady,
            Error::InvalidArgument { .. } => ErrorCode::InvalidArgument,
            Error::NotFound { .. } => ErrorCode::NotFound,
            Error::Forbidden { .. } => ErrorCode::Forbidden,
            Error::IpcNoHandler { .. } => ErrorCode::IpcNoHandler,
            Error::IpcTimeout { .. } => ErrorCode::IpcTimeout,
            Error::IpcSerialization { .. } => ErrorCode::IpcSerialization,
            Error::IpcRemoteError { .. } => ErrorCode::IpcRemoteError,
            Error::IpcOverloaded { .. } => ErrorCode::IpcOverloaded,
            Error::Io { .. } => ErrorCode::Io,
            Error::Network { .. } => ErrorCode::Network,
            Error::Backend { .. } => ErrorCode::Backend,
        }
    }

    /// The human-readable message.
    #[must_use]
    pub fn message(&self) -> &str {
        self.parts().0
    }

    /// The code-specific details, if any.
    #[must_use]
    pub fn data(&self) -> Option<&Value> {
        self.parts().1.as_ref()
    }

    fn parts(&self) -> (&str, &Option<Value>) {
        match self {
            Error::Unsupported { message, data }
            | Error::NotReady { message, data }
            | Error::InvalidArgument { message, data }
            | Error::NotFound { message, data }
            | Error::Forbidden { message, data }
            | Error::IpcNoHandler { message, data }
            | Error::IpcTimeout { message, data }
            | Error::IpcSerialization { message, data }
            | Error::IpcRemoteError { message, data }
            | Error::IpcOverloaded { message, data }
            | Error::Io { message, data }
            | Error::Network { message, data }
            | Error::Backend { message, data } => (message, data),
        }
    }

    /// Wraps an I/O failure. The OS message is kept in `data.os` and the
    /// path is never included.
    #[must_use]
    pub fn from_io(context: &str, err: &std::io::Error) -> Self {
        Error::Io {
            message: format!("{context} failed."),
            data: Some(serde_json::json!({ "os": err.kind().to_string() })),
        }
    }
}

impl Serialize for Error {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (message, data) = self.parts();
        let mut s = serializer.serialize_struct("OverwolfErrorWire", 3)?;
        s.serialize_field("code", self.code().as_str())?;
        s.serialize_field("message", message)?;
        if let Some(data) = data {
            s.serialize_field("data", data)?;
        } else {
            s.skip_field("data")?;
        }
        s.end()
    }
}

#[cfg(feature = "plugin")]
impl From<tauri::Error> for Error {
    fn from(err: tauri::Error) -> Self {
        Error::Io {
            message: "The window system reported an error.".into(),
            data: Some(serde_json::json!({ "raw": err.to_string() })),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Error, ErrorCode};

    /// The codes of `OwTauriErrorCode` in `packages/ow-tauri/src/shared/errors.ts`.
    const TS_CODES: [&str; 13] = [
        "unsupported",
        "not-ready",
        "invalid-argument",
        "not-found",
        "forbidden",
        "ipc-no-handler",
        "ipc-timeout",
        "ipc-serialization",
        "ipc-remote-error",
        "ipc-overloaded",
        "io",
        "network",
        "backend",
    ];

    #[test]
    fn codes_match_the_typescript_union() {
        let rust: Vec<&str> = ErrorCode::ALL.iter().map(|c| c.as_str()).collect();
        assert_eq!(rust, TS_CODES);
    }

    #[test]
    fn typescript_source_lists_every_code() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../packages/ow-tauri/src/shared/errors.ts"
        );
        // The crate can be built outside the repository (crates.io); skip then.
        let Ok(source) = std::fs::read_to_string(path) else {
            return;
        };
        for code in TS_CODES {
            assert!(
                source.contains(&format!("'{code}'")),
                "errors.ts lacks {code}"
            );
        }
    }

    #[test]
    fn every_code_round_trips_through_new() {
        for code in ErrorCode::ALL {
            let err = Error::new(code, "m.", None);
            assert_eq!(err.code(), code);
            assert_eq!(err.message(), "m.");
            assert!(err.data().is_none());
            let wire = serde_json::to_value(&err).unwrap();
            assert_eq!(wire["code"], code.as_str());
        }
    }

    #[test]
    fn data_is_serialised_when_present() {
        let err = Error::invalid_argument("Bad channel.").with_data(serde_json::json!({"x": 1}));
        let wire = serde_json::to_value(&err).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({"code": "invalid-argument", "message": "Bad channel.", "data": {"x": 1}})
        );
        assert_eq!(err.to_string(), "Bad channel.");
    }

    #[test]
    fn io_errors_keep_no_path() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "/secret/path missing");
        let err = Error::from_io("Reading the file", &io);
        let text = serde_json::to_string(&err).unwrap();
        assert!(!text.contains("secret"));
        assert_eq!(err.code(), ErrorCode::Io);
    }
}
