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

/// The machine-id muid (E.4, matches ow-electron, observed on macOS): the
/// lower-case hex SHA-256 of the lower-cased platform id, cut into the
/// 8-4-4-4-12 UUID layout, with no version or variant bits forced.
///
/// ```
/// use tauri_plugin_overwolf::identity::{machine_muid, phase_percent};
/// let m = machine_muid("2D59BF70-9641-826A-F003-C362834EC045");
/// assert_eq!(m, "5bd79133-f3bf-be27-e448-a4581ab5f3cd");
/// assert_eq!(phase_percent(&m), 80);
/// ```
#[must_use]
pub fn machine_muid(platform_id: &str) -> String {
    let digest = sha2::Sha256::digest(platform_id.trim().to_lowercase().as_bytes());
    let hex: String = digest.iter().fold(String::with_capacity(64), |mut s, b| {
        let _ = write!(s, "{b:02x}");
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

/// `overwolf.EmailHashes`: `{ sha1?, sha256?, md5? }`, serialised in
/// ow-electron's key order `sha1`, `md5`, `sha256` (A.2.2).
///
/// ```
/// use tauri_plugin_overwolf::identity::{email_hashes, HashEncoding};
/// let json = serde_json::to_string(&email_hashes("a@b.c", HashEncoding::Hex)).unwrap();
/// let (sha1, md5, sha256) = (json.find("sha1").unwrap(), json.find("md5").unwrap(), json.find("sha256").unwrap());
/// assert!(sha1 < md5 && md5 < sha256);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailHashes {
    /// SHA-1 of the normalised address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    /// MD5 of the normalised address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub md5: Option<String>,
    /// SHA-256 of the normalised address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

impl EmailHashes {
    /// Whether every field is absent or empty (which clears stored hashes).
    ///
    /// ```
    /// use tauri_plugin_overwolf::identity::EmailHashes;
    /// assert!(EmailHashes::default().is_empty());
    /// let some = EmailHashes { md5: Some("abc".into()), ..EmailHashes::default() };
    /// assert!(!some.is_empty());
    /// ```
    #[must_use]
    pub fn is_empty(&self) -> bool {
        [&self.sha1, &self.md5, &self.sha256]
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
        md5: Some(encode(&md5)),
        sha256: Some(encode(&sha256)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// The formula itself on CONTRACT G.2 vector 1 (the full table runs
    /// through `app_identity`).
    #[test]
    fn formula() {
        assert_eq!(
            computed_uid("Example Studio", "parity-harness"),
            "binaioonkjpolnojeenpbmjmbfkbmffcekndbmdk"
        );
    }

    #[test]
    fn machine_muid_vectors() {
        for (id, muid, phase) in [
            (
                "2D59BF70-9641-826A-F003-C362834EC045",
                "5bd79133-f3bf-be27-e448-a4581ab5f3cd",
                80,
            ),
            (
                "DA3889E5-CB8A-8A15-CD1B-DCE6B5A71203",
                "601860a3-90c7-b77b-a42e-636035921a81",
                51,
            ),
            (
                "D668AFF2-C8FD-39A3-6B92-D57DED8E5461",
                "5d841b98-54cb-5f57-73bc-297706f34221",
                62,
            ),
            (
                "58468E7A-3E77-8816-5D62-7371174B102C",
                "cbec68c3-9e97-b465-f479-f8493f973f32",
                24,
            ),
        ] {
            assert_eq!(machine_muid(id), muid, "{id}");
            assert_eq!(machine_muid(&id.to_lowercase()), muid, "case-insensitive");
            assert_eq!(phase_percent(muid), phase, "{muid}");
        }
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
