//! App and machine identity: uid, cuid, muid, phase percent and email hashes
//! (CONTRACT G.2, E.4, A.2.2).
//!
//! Every function here is pure, so the JavaScript side can implement the same
//! rules and both are checked against shared vectors.
//!
//! ```
//! use tauri_plugin_overwolf::identity::computed_uid;
//! // The upstream sample's identity (author "Overwolf Ltd.", app name from build.productName).
//! assert_eq!(
//!     computed_uid("Overwolf Ltd.", "Overwolf Electron Official Sample App"),
//!     "djpddhibpjddgdpcfkbooljealnjnamkhlihgbab",
//! );
//! ```

use std::fmt::Write as _;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::Digest as _;

use crate::manifest::EmbeddedManifest;

/// Computes the ow-electron app uid from the author and the app name (G.2 rule 3).
///
/// `sha1("{'author':'<author>','name':'<name>.electron'}")`; each digest byte
/// `b` becomes the two characters `'a' + (b & 15)` then `'a' + (b >> 4)`,
/// giving 40 characters in `a`..`p`.
///
/// ```
/// let uid = tauri_plugin_overwolf::identity::computed_uid("Example Studio", "Example App");
/// assert_eq!(uid.len(), 40);
/// assert!(uid.bytes().all(|b| (b'a'..=b'p').contains(&b)));
/// ```
#[must_use]
pub fn computed_uid(author: &str, name: &str) -> String {
    let input = format!("{{'author':'{author}','name':'{name}.electron'}}");
    let digest = sha1_smol::Sha1::from(input.as_bytes()).digest().bytes();
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        out.push(char::from(b'a' + (b & 0x0f)));
        out.push(char::from(b'a' + (b >> 4)));
    }
    out
}

/// Where the effective uid came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UidSource {
    /// `plugins.overwolf.uid` or a `Builder` override.
    Config,
    /// `overwolf.uid` in the manifest (console-signed builds).
    Manifest,
    /// The formula of [`computed_uid`].
    Computed,
}

/// The app's uid and cuid (G.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppIdentity {
    /// The effective uid.
    pub uid: String,
    /// Always the computed value, even when an override applies.
    pub cuid: String,
    /// Which rule produced `uid`.
    pub source: UidSource,
}

/// Resolves the uid by the G.2 precedence: configured override, then the
/// manifest's `overwolf.uid`, then the computed value.
///
/// ```
/// use tauri_plugin_overwolf::identity::{resolve_uid, UidSource};
/// use tauri_plugin_overwolf::manifest::EmbeddedManifest;
/// let m = EmbeddedManifest::minimal("Example App", "Example Studio", "1.0.0");
/// let id = resolve_uid(Some("aaaabbbbccccddddeeeeffffgggghhhhiiiijjjj"), &m);
/// assert_eq!(id.source, UidSource::Config);
/// assert_ne!(id.cuid, id.uid);
/// ```
#[must_use]
pub fn resolve_uid(config_uid: Option<&str>, manifest: &EmbeddedManifest) -> AppIdentity {
    let cuid = computed_uid(&manifest.author, &manifest.product_name);
    let config_uid = config_uid.map(str::trim).filter(|s| !s.is_empty());
    let manifest_uid = manifest
        .overwolf
        .uid
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    match (config_uid, manifest_uid) {
        (Some(uid), _) => AppIdentity {
            uid: uid.to_owned(),
            cuid,
            source: UidSource::Config,
        },
        (None, Some(uid)) => AppIdentity {
            uid: uid.to_owned(),
            cuid,
            source: UidSource::Manifest,
        },
        (None, None) => AppIdentity {
            uid: cuid.clone(),
            cuid,
            source: UidSource::Computed,
        },
    }
}

/// Whether `uid` is acceptable as a configured uid: 1 to 64 ASCII letters or
/// digits. Console uids are 40 characters in `a`..`p`.
///
/// ```
/// use tauri_plugin_overwolf::identity::is_valid_uid;
/// assert!(is_valid_uid("djpddhibpjddgdpcfkbooljealnjnamkhlihgbab"));
/// assert!(!is_valid_uid("../x"));
/// assert!(!is_valid_uid(""));
/// ```
#[must_use]
pub fn is_valid_uid(uid: &str) -> bool {
    !uid.is_empty() && uid.len() <= 64 && uid.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// The phase percent of a muid (E.4): the sum of the character codes of the
/// lower-case hex MD5 of the muid without `-`, modulo 100.
///
/// ```
/// use tauri_plugin_overwolf::identity::phase_percent;
/// assert_eq!(phase_percent("0123abcd-0000-4000-8000-00000000abcd"), 50);
/// ```
#[must_use]
pub fn phase_percent(muid: &str) -> u8 {
    let hex = format!("{:x}", md5::compute(muid.replace('-', "").as_bytes()));
    let sum: u32 = hex.bytes().map(u32::from).sum();
    // Lossless: the value is below 100.
    u8::try_from(sum % 100).unwrap_or(0)
}

/// Whether `s` is a muid in the `per-install` format: a hyphenated UUID
/// (36 characters), upper-case hex.
///
/// ```
/// use tauri_plugin_overwolf::identity::is_valid_muid;
/// assert!(is_valid_muid("8C7E4F2A-0000-4000-8000-00000000ABCD"));
/// assert!(!is_valid_muid("8c7e4f2a-0000-4000-8000-00000000abcd"));
/// ```
#[must_use]
pub fn is_valid_muid(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(|(i, &c)| match i {
            8 | 13 | 18 | 23 => c == b'-',
            _ => c.is_ascii_digit() || (b'A'..=b'F').contains(&c),
        })
}

/// Formats 16 random bytes as an upper-case UUID v4 muid (E.4 `per-install`).
///
/// ```
/// use tauri_plugin_overwolf::identity::{muid_from_bytes, is_valid_muid};
/// let m = muid_from_bytes([0xab; 16]);
/// assert!(is_valid_muid(&m));
/// assert_eq!(&m[14..15], "4", "version nibble");
/// ```
#[must_use]
pub fn muid_from_bytes(mut bytes: [u8; 16]) -> String {
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().fold(String::with_capacity(32), |mut s, b| {
        let _ = write!(s, "{b:02X}");
        s
    });
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// Encoding of email hashes (`emailHashes.encoding`, Interim OQ-10).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HashEncoding {
    /// Lower-case hexadecimal (default).
    #[default]
    Hex,
    /// Standard base64 with padding.
    Base64,
}

/// `overwolf.EmailHashes`: `{ sha1?, sha256?, md5? }`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailHashes {
    /// SHA-1 of the normalised address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    /// SHA-256 of the normalised address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// MD5 of the normalised address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub md5: Option<String>,
}

impl EmailHashes {
    /// Whether every field is absent or empty (which clears stored hashes).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        [&self.sha1, &self.sha256, &self.md5]
            .iter()
            .all(|h| h.as_deref().is_none_or(str::is_empty))
    }
}

/// Normalises an email address by the UID2 rules the ow-electron typings
/// link to: trim, lower-case, and for `gmail.com` remove `.` and any
/// `+suffix` from the local part. Empty input returns `None`.
///
/// ```
/// use tauri_plugin_overwolf::identity::normalize_email;
/// assert_eq!(normalize_email(" Jane.Doe+ads@GMAIL.com ").as_deref(), Some("janedoe@gmail.com"));
/// assert_eq!(normalize_email("a.b+c@example.com").as_deref(), Some("a.b+c@example.com"));
/// assert_eq!(normalize_email("   "), None);
/// ```
#[must_use]
pub fn normalize_email(email: &str) -> Option<String> {
    let lower = email.trim().to_lowercase();
    if lower.is_empty() {
        return None;
    }
    match lower.rsplit_once('@') {
        Some((local, "gmail.com")) => {
            let local = local.split_once('+').map_or(local, |(head, _)| head);
            Some(format!("{}@gmail.com", local.replace('.', "")))
        }
        _ => Some(lower),
    }
}

/// `generateUserEmailHashes(email)`: hashes the normalised address
/// (CONTRACT A.2.2). Empty or whitespace input returns empty hashes.
///
/// ```
/// use tauri_plugin_overwolf::identity::{email_hashes, HashEncoding};
/// let h = email_hashes("user@example.com", HashEncoding::Hex);
/// assert_eq!(h.md5.as_deref(), Some("b58996c504c5638798eb6b511e6f49af"));
/// assert!(email_hashes("  ", HashEncoding::Hex).is_empty());
/// ```
#[must_use]
pub fn email_hashes(email: &str, encoding: HashEncoding) -> EmailHashes {
    let Some(normalized) = normalize_email(email) else {
        return EmailHashes::default();
    };
    let bytes = normalized.as_bytes();
    let sha1 = sha1_smol::Sha1::from(bytes).digest().bytes().to_vec();
    let sha256 = sha2::Sha256::digest(bytes).to_vec();
    let md5 = md5::compute(bytes).0.to_vec();
    let encode = |raw: &[u8]| match encoding {
        HashEncoding::Hex => raw
            .iter()
            .fold(String::with_capacity(raw.len() * 2), |mut s, b| {
                let _ = write!(s, "{b:02x}");
                s
            }),
        HashEncoding::Base64 => base64::engine::general_purpose::STANDARD.encode(raw),
    };
    EmailHashes {
        sha1: Some(encode(&sha1)),
        sha256: Some(encode(&sha256)),
        md5: Some(encode(&md5)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::EmbeddedManifest;

    #[test]
    fn uid_vectors() {
        // (author, app name, uid): the upstream sample with build.productName and
        // with its npm name, a neutral example, and the empty edge case.
        let vectors = [
            (
                "Overwolf Ltd.",
                "Overwolf Electron Official Sample App",
                "djpddhibpjddgdpcfkbooljealnjnamkhlihgbab",
            ),
            (
                "Overwolf Ltd.",
                "overwolf-official-sample-app",
                "nihmfahfaloahhjpignlkjlkfemlnlnndmgjcfbh",
            ),
            (
                "Example Studio",
                "Example App",
                "cbcpjpfokndifakfgkfmgepmdbhaipkabdadkjnn",
            ),
            ("", "", "infekmdkkifnlpbanpiolmenifiddcgfchceimhc"),
        ];
        for (author, name, uid) in vectors {
            assert_eq!(computed_uid(author, name), uid, "{author} / {name}");
        }
    }

    #[test]
    fn uid_precedence() {
        let mut m = EmbeddedManifest::minimal("Example App", "Example Studio", "1.0.0");
        let computed = computed_uid("Example Studio", "Example App");
        let id = resolve_uid(None, &m);
        assert_eq!(
            (id.uid.as_str(), id.source),
            (computed.as_str(), UidSource::Computed)
        );
        m.overwolf.uid = Some("manifestuid".into());
        let id = resolve_uid(None, &m);
        assert_eq!(
            (id.uid.as_str(), id.source),
            ("manifestuid", UidSource::Manifest)
        );
        let id = resolve_uid(Some("configuid"), &m);
        assert_eq!(
            (id.uid.as_str(), id.source),
            ("configuid", UidSource::Config)
        );
        assert_eq!(id.cuid, computed, "cuid is always computed");
        let id = resolve_uid(Some("  "), &m);
        assert_eq!(id.source, UidSource::Manifest, "blank override is ignored");
    }

    #[test]
    fn phase_vectors() {
        assert_eq!(phase_percent("0123abcd-0000-4000-8000-00000000abcd"), 50);
        assert_eq!(phase_percent("0123ABCD-0000-4000-8000-00000000ABCD"), 45);
        assert_eq!(phase_percent("00000000-0000-0000-0000-000000000000"), 62);
        for i in 0..=255u8 {
            assert!(phase_percent(&muid_from_bytes([i; 16])) < 100);
        }
    }

    #[test]
    fn muid_format() {
        let m = muid_from_bytes([0u8; 16]);
        assert_eq!(m, "00000000-0000-4000-8000-000000000000");
        assert!(is_valid_muid(&m));
        assert!(!is_valid_muid("00000000-0000-4000-8000-00000000000"));
        assert!(!is_valid_muid("00000000+0000-4000-8000-000000000000"));
        assert!(!is_valid_muid("0000000G-0000-4000-8000-000000000000"));
    }

    #[test]
    fn uid_validation() {
        assert!(is_valid_uid("abc123"));
        assert!(!is_valid_uid(&"a".repeat(65)));
        assert!(!is_valid_uid("a b"));
    }

    #[derive(serde::Deserialize)]
    struct Fixture {
        vectors: Vec<Vector>,
    }

    #[derive(serde::Deserialize)]
    struct Vector {
        email: String,
        normalized: Option<String>,
        hex: EmailHashes,
        base64: EmailHashes,
    }

    #[test]
    fn email_hash_fixture() {
        let text = include_str!("../tests/fixtures/email-hashes.json");
        let fixture: Fixture = serde_json::from_str(text).unwrap();
        assert!(fixture.vectors.len() >= 5);
        for v in fixture.vectors {
            assert_eq!(normalize_email(&v.email), v.normalized, "{:?}", v.email);
            assert_eq!(
                email_hashes(&v.email, HashEncoding::Hex),
                v.hex,
                "{:?}",
                v.email
            );
            assert_eq!(
                email_hashes(&v.email, HashEncoding::Base64),
                v.base64,
                "{:?}",
                v.email
            );
        }
    }

    #[test]
    fn empty_hashes() {
        assert!(EmailHashes::default().is_empty());
        let h = EmailHashes {
            sha1: Some(String::new()),
            ..EmailHashes::default()
        };
        assert!(h.is_empty());
        assert!(!email_hashes("a@b.c", HashEncoding::Hex).is_empty());
        assert_eq!(
            serde_json::to_value(EmailHashes::default()).unwrap(),
            serde_json::json!({})
        );
    }
}
