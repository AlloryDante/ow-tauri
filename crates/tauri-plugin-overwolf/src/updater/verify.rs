//! Download verification (CONTRACT I.3, [ADR 0008](https://github.com/ow-tauri/ow-tauri/blob/main/docs/adr/0008-updater-client.md)):
//! the SHA-512 of the feed entry, the detached minisign signature, and the
//! pieces of the OS publisher checks that need no OS: the Authenticode
//! report of PowerShell's `Get-AuthenticodeSignature`, electron-updater's
//! publisher-name rule and the macOS team identifier.
//!
//! Every check fails closed: anything unreadable is a failure.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Read;
use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use sha2::{Digest, Sha512};

use crate::error::Error;

fn backend(message: &str) -> Error {
    Error::backend(message.to_owned())
}

/// Whether `actual` (the raw SHA-512 of the download) is the feed's
/// `expected` value: base64 as electron-builder writes it, or 128 hex
/// digits.
///
/// ```
/// use sha2::{Digest, Sha512};
/// use tauri_plugin_overwolf::updater::verify::sha512_matches;
/// let digest = Sha512::digest(b"test");
/// let b64 = "7iaw3Ur350mqGo7jwQrpkj9hiYB3Lkc/iBml1JQODbJ6wYX4oOHV+E+IvIh/1nsUNzLDBMxfqa2Ob1f1ACio/w==";
/// assert!(sha512_matches(b64, &digest));
/// assert!(!sha512_matches("", &digest));
/// ```
#[must_use]
pub fn sha512_matches(expected: &str, actual: &[u8]) -> bool {
    let expected = expected.trim();
    if expected.is_empty() {
        return false;
    }
    if expected.len() == 128 && expected.bytes().all(|b| b.is_ascii_hexdigit()) {
        let hex: String = actual.iter().fold(String::new(), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        });
        return hex.eq_ignore_ascii_case(expected);
    }
    STANDARD
        .decode(expected)
        .is_ok_and(|bytes| bytes.as_slice() == actual)
}

/// SHA-512 of a file.
///
/// # Errors
///
/// `io` when the file cannot be read.
pub fn sha512_file(path: &Path) -> Result<Vec<u8>, Error> {
    let mut file =
        std::fs::File::open(path).map_err(|e| Error::from_io("Reading the update file", &e))?;
    let mut hasher = Sha512::new();
    let mut buf = vec![0_u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| Error::from_io("Reading the update file", &e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_vec())
}

/// Reads a minisign text that may itself be base64 (the form
/// `tauri-plugin-updater` stores keys and `.sig` files in).
fn unwrap_base64_text(text: &str) -> String {
    let t = text.trim();
    if t.contains('\n') || t.starts_with("untrusted comment:") {
        return t.to_owned();
    }
    match STANDARD.decode(t) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(inner) if inner.contains("untrusted comment:") => inner.trim().to_owned(),
            _ => t.to_owned(),
        },
        Err(_) => t.to_owned(),
    }
}

/// Parses `updater.pubkey`: a minisign public key as its bare base64 line,
/// the two-line key file, or the base64 of that file (`tauri-plugin-updater`
/// style).
///
/// # Errors
///
/// `invalid-argument` when it is none of these.
///
/// ```
/// use tauri_plugin_overwolf::updater::verify::parse_public_key;
/// assert!(parse_public_key("RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3").is_ok());
/// assert!(parse_public_key("not a key").is_err());
/// ```
pub fn parse_public_key(text: &str) -> Result<minisign_verify::PublicKey, Error> {
    let t = unwrap_base64_text(text);
    let parsed = if t.contains('\n') {
        minisign_verify::PublicKey::decode(&t)
    } else {
        minisign_verify::PublicKey::from_base64(&t)
    };
    parsed.map_err(|_| Error::invalid_argument("updater.pubkey is not a minisign public key."))
}

/// Verifies the detached minisign signature `signature` (the `.sig` file,
/// plain or base64) of the file at `path` with `pubkey`. Only prehashed
/// (`BLAKE2b`) signatures are accepted, as `tauri-plugin-updater` writes.
///
/// # Errors
///
/// `backend` when the signature does not parse or does not verify, `io`
/// when the file cannot be read.
pub fn verify_minisign(
    pubkey: &minisign_verify::PublicKey,
    signature: &str,
    path: &Path,
) -> Result<(), Error> {
    let sig = minisign_verify::Signature::decode(&unwrap_base64_text(signature))
        .map_err(|_| backend("The update signature file does not parse."))?;
    let mut verifier = pubkey
        .verify_stream(&sig)
        .map_err(|_| backend("The update signature was made with another key."))?;
    let mut file =
        std::fs::File::open(path).map_err(|e| Error::from_io("Reading the update file", &e))?;
    let mut buf = vec![0_u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| Error::from_io("Reading the update file", &e))?;
        if n == 0 {
            break;
        }
        verifier.update(&buf[..n]);
    }
    verifier
        .finalize()
        .map_err(|_| backend("The update signature does not match the file."))
}

/// An RFC 2253 distinguished name as electron-updater's `parseDn` reads it:
/// attribute type to value. A string without `=` is not a DN (empty map).
///
/// ```
/// use tauri_plugin_overwolf::updater::verify::parse_dn;
/// let dn = parse_dn(r#"CN="Studio, Inc.", O=Studio\, Inc., C=US"#);
/// assert_eq!(dn.get("CN").map(String::as_str), Some("Studio, Inc."));
/// assert_eq!(dn.get("O").map(String::as_str), Some("Studio, Inc."));
/// assert!(parse_dn("Studio Inc").is_empty());
/// ```
#[must_use]
pub fn parse_dn(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut key = String::new();
    let mut value = String::new();
    let mut in_value = false;
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    let push = |key: &mut String, value: &mut String, out: &mut BTreeMap<String, String>| {
        let k = key.trim();
        if !k.is_empty() {
            out.insert(k.to_owned(), value.trim().to_owned());
        }
        key.clear();
        value.clear();
    };
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' => quoted = false,
                '\\' => {
                    if let Some(n) = chars.next() {
                        value.push(n);
                    }
                }
                _ => value.push(c),
            }
            continue;
        }
        match c {
            '\\' => {
                if let Some(n) = chars.next() {
                    if in_value { value.push(n) } else { key.push(n) }
                }
            }
            '"' if in_value && value.trim().is_empty() => {
                value.clear();
                quoted = true;
            }
            '=' if !in_value => in_value = true,
            ',' | ';' | '+' if in_value => {
                push(&mut key, &mut value, &mut out);
                in_value = false;
            }
            _ if in_value => value.push(c),
            _ => key.push(c),
        }
    }
    if in_value {
        push(&mut key, &mut value, &mut out);
    }
    out
}

/// electron-updater's publisher rule: a name that is a DN must match every
/// attribute it lists; any other name must equal the subject's `CN`.
///
/// ```
/// use tauri_plugin_overwolf::updater::verify::publisher_matches;
/// let subject = "CN=Studio Inc, O=Studio Inc, L=Lisbon, C=PT";
/// assert!(publisher_matches(subject, &["Studio Inc".to_owned()]));
/// assert!(publisher_matches(subject, &["CN=Studio Inc, C=PT".to_owned()]));
/// assert!(!publisher_matches(subject, &["CN=Studio Inc, C=US".to_owned()]));
/// assert!(!publisher_matches(subject, &["Other".to_owned()]));
/// ```
#[must_use]
pub fn publisher_matches(subject: &str, publisher_names: &[String]) -> bool {
    let subject = parse_dn(subject);
    publisher_names.iter().any(|name| {
        let dn = parse_dn(name);
        if dn.is_empty() {
            subject.get("CN").is_some_and(|cn| cn == name)
        } else {
            dn.iter().all(|(k, v)| subject.get(k) == Some(v))
        }
    })
}

/// The parts of `Get-AuthenticodeSignature | ConvertTo-Json -Compress`
/// the update client reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authenticode {
    /// `Status`: 0 is `Valid`.
    pub status: i64,
    /// `StatusMessage`.
    pub status_message: String,
    /// `SignerCertificate.Subject`.
    pub subject: Option<String>,
    /// `Path`: the file PowerShell checked.
    pub path: String,
}

impl Authenticode {
    /// Whether the signature is valid (`Status` 0).
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.status == 0
    }
}

/// Parses PowerShell's JSON report (UTF-8, maybe with a BOM).
///
/// # Errors
///
/// `backend` when the text is not such a report.
///
/// ```
/// use tauri_plugin_overwolf::updater::verify::parse_authenticode;
/// let r = parse_authenticode(r#"{"SignerCertificate":{"Subject":"CN=Studio"},"Status":0,"StatusMessage":"Signature verified.","Path":"C:\\t\\setup.exe"}"#).unwrap();
/// assert!(r.is_valid());
/// assert_eq!(r.subject.as_deref(), Some("CN=Studio"));
/// ```
pub fn parse_authenticode(text: &str) -> Result<Authenticode, Error> {
    let t = text.trim_start_matches('\u{feff}').trim();
    let v: serde_json::Value =
        serde_json::from_str(t).map_err(|_| backend("The signature check gave no report."))?;
    let status = v
        .get("Status")
        .and_then(serde_json::Value::as_i64)
        .ok_or_else(|| backend("The signature check report has no status."))?;
    Ok(Authenticode {
        status,
        status_message: v
            .get("StatusMessage")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        subject: v
            .get("SignerCertificate")
            .and_then(|c| c.get("Subject"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        path: v
            .get("Path")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    })
}

/// The decision of the Windows publisher check (I.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublisherDecision {
    /// The installer is signed by an accepted publisher.
    Accept,
    /// No `publisherNames` and the running executable is unsigned: the
    /// check is skipped (warn once).
    SkipUnsigned,
    /// No `publisherNames` and the app exe is signed with Overwolf's
    /// certificate (`enableOWCertSigning`), so its subject is not the
    /// installer's publisher: the check is skipped (warn once), as
    /// electron-updater skips it without a `publisherName`.
    SkipNoPublisher,
    /// The installer fails the check; the message says why.
    Reject(&'static str),
}

/// Applies the I.3 Windows rule to the reports of the running executable
/// (`own`) and the downloaded installer.
///
/// - Configured `updater.publisherNames` (`Some`) are always enforced, as
///   electron-updater enforces `publisherName`.
/// - Without them, an app exe signed with Overwolf's certificate
///   (`ow_certificate`: `build.overwolf.enableOWCertSigning`) skips the
///   check, since Overwolf's subject never signs the installer.
/// - Otherwise the running executable's own subject is the publisher; an
///   unsigned running executable skips the check.
///
/// ```
/// use tauri_plugin_overwolf::updater::verify::{decide_publisher, Authenticode, PublisherDecision};
/// let signed = |s: &str| Authenticode { status: 0, status_message: String::new(), subject: Some(s.into()), path: String::new() };
/// let unsigned = Authenticode { status: 2, status_message: String::new(), subject: None, path: String::new() };
/// assert_eq!(decide_publisher(&signed("CN=A"), &signed("CN=A"), None, false), PublisherDecision::Accept);
/// assert_eq!(decide_publisher(&unsigned, &unsigned, None, false), PublisherDecision::SkipUnsigned);
/// assert!(matches!(decide_publisher(&signed("CN=A"), &signed("CN=B"), None, false), PublisherDecision::Reject(_)));
/// assert!(matches!(decide_publisher(&signed("CN=A"), &unsigned, None, false), PublisherDecision::Reject(_)));
/// // Overwolf's certificate on the app exe, the developer's on the installer.
/// let ow = signed("CN=Overwolf Ltd");
/// assert_eq!(decide_publisher(&ow, &signed("CN=Studio"), None, true), PublisherDecision::SkipNoPublisher);
/// let names = ["Studio".to_owned()];
/// assert_eq!(decide_publisher(&ow, &signed("CN=Studio"), Some(&names), true), PublisherDecision::Accept);
/// ```
#[must_use]
pub fn decide_publisher(
    own: &Authenticode,
    installer: &Authenticode,
    publisher_names: Option<&[String]>,
    ow_certificate: bool,
) -> PublisherDecision {
    if publisher_names.is_none() {
        if ow_certificate {
            return PublisherDecision::SkipNoPublisher;
        }
        if !own.is_valid() {
            return PublisherDecision::SkipUnsigned;
        }
    }
    if !installer.is_valid() {
        return PublisherDecision::Reject("The update installer has no valid signature.");
    }
    let Some(subject) = installer.subject.as_deref() else {
        return PublisherDecision::Reject("The update installer's signer is unknown.");
    };
    let own_subject;
    let names: &[String] = if let Some(names) = publisher_names {
        names
    } else {
        let Some(s) = own.subject.clone() else {
            return PublisherDecision::Reject("The running app's signer is unknown.");
        };
        own_subject = [s];
        &own_subject
    };
    // A name that is a DN (the default, the running exe's subject) must
    // match every attribute; a plain name must equal the CN.
    let accepted = publisher_matches(subject, names);
    if accepted {
        PublisherDecision::Accept
    } else {
        PublisherDecision::Reject("The update installer is signed by another publisher.")
    }
}

/// The designated requirement in the output of `codesign -d -r-` (macOS):
/// the text after `designated =>`, also when `codesign` marks it implicit
/// with a leading `#`. Squirrel.Mac requires an update to satisfy the
/// running app's designated requirement, which names its bundle
/// identifier and signer.
///
/// ```
/// use tauri_plugin_overwolf::updater::verify::designated_requirement;
/// let out = "designated => identifier \"com.example.app\" and anchor apple generic\n";
/// assert_eq!(
///     designated_requirement(out).as_deref(),
///     Some("identifier \"com.example.app\" and anchor apple generic")
/// );
/// assert_eq!(designated_requirement("# designated => cdhash H\"00\"").as_deref(), Some("cdhash H\"00\""));
/// assert_eq!(designated_requirement("Executable=/x\n"), None);
/// ```
#[must_use]
pub fn designated_requirement(codesign_output: &str) -> Option<String> {
    codesign_output
        .lines()
        .find_map(|l| {
            l.trim()
                .trim_start_matches('#')
                .trim_start()
                .strip_prefix("designated =>")
        })
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(str::to_owned)
}

/// The `TeamIdentifier` in the output of `codesign -dv` (macOS); `None`
/// when absent or `not set`.
///
/// ```
/// use tauri_plugin_overwolf::updater::verify::team_identifier;
/// assert_eq!(team_identifier("Identifier=x\nTeamIdentifier=AB12CD34EF\n").as_deref(), Some("AB12CD34EF"));
/// assert_eq!(team_identifier("TeamIdentifier=not set\n"), None);
/// ```
#[must_use]
pub fn team_identifier(codesign_output: &str) -> Option<String> {
    codesign_output
        .lines()
        .find_map(|l| l.trim().strip_prefix("TeamIdentifier="))
        .map(str::trim)
        .filter(|t| !t.is_empty() && *t != "not set")
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
    /// A prehashed minisign signature of the four bytes `test` (the
    /// minisign-verify crate's own test vector).
    const SIG: &str = "untrusted comment: signature from minisign secret key
RUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=
trusted comment: timestamp:1556193335\tfile:test
y/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg==";

    fn temp_file(name: &str, data: &[u8]) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ow-tauri-verify-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("file.bin");
        std::fs::write(&p, data).unwrap();
        p
    }

    #[test]
    fn minisign_plain_and_base64_forms() {
        let file = temp_file("ok", b"test");
        let key_file = format!("untrusted comment: minisign public key\n{KEY}\n");
        for key in [KEY.to_owned(), key_file.clone(), STANDARD.encode(&key_file)] {
            let pk = parse_public_key(&key).unwrap();
            verify_minisign(&pk, SIG, &file).unwrap();
            verify_minisign(&pk, &STANDARD.encode(SIG), &file).unwrap();
        }
        let pk = parse_public_key(KEY).unwrap();
        let other = temp_file("bad", b"Test");
        assert_eq!(
            verify_minisign(&pk, SIG, &other).unwrap_err().code(),
            crate::ErrorCode::Backend
        );
        assert!(verify_minisign(&pk, "garbage", &file).is_err());
        let missing = file.with_file_name("missing.bin");
        assert_eq!(
            verify_minisign(&pk, SIG, &missing).unwrap_err().code(),
            crate::ErrorCode::Io
        );
    }

    #[test]
    fn sha512_forms() {
        let file = temp_file("sha", b"test");
        let digest = sha512_file(&file).unwrap();
        let hex = digest.iter().fold(String::new(), |mut s, b| {
            let _ = write!(s, "{b:02X}");
            s
        });
        assert!(sha512_matches(&hex, &digest));
        assert!(sha512_matches(&STANDARD.encode(&digest), &digest));
        assert!(!sha512_matches("!!", &digest));
        assert!(!sha512_matches(&STANDARD.encode(b"other"), &digest));
    }

    #[test]
    fn dn_parsing_edge_cases() {
        let dn = parse_dn(r#" CN = Studio ; O="Q \"x\"" + OU=Dev\+Ops, E=a@b.c"#);
        assert_eq!(dn["CN"], "Studio");
        assert_eq!(dn["O"], "Q \"x\"");
        assert_eq!(dn["OU"], "Dev+Ops");
        assert_eq!(dn["E"], "a@b.c");
        assert_eq!(parse_dn("CN=").get("CN").map(String::as_str), Some(""));
    }

    #[test]
    fn publisher_decisions() {
        let signed = |s: &str| Authenticode {
            status: 0,
            status_message: String::new(),
            subject: Some(s.into()),
            path: String::new(),
        };
        let own = signed("CN=Studio Inc, O=Studio Inc, C=PT");
        // Default: the running executable's full subject.
        assert_eq!(
            decide_publisher(
                &own,
                &signed("CN=Studio Inc, O=Studio Inc, C=PT"),
                None,
                false
            ),
            PublisherDecision::Accept
        );
        assert!(matches!(
            decide_publisher(&own, &signed("CN=Studio Inc, O=Other, C=PT"), None, false),
            PublisherDecision::Reject(_)
        ));
        // Configured names: CN or DN, as electron-updater.
        let names = ["Studio Inc".to_owned()];
        assert_eq!(
            decide_publisher(
                &own,
                &signed("CN=Studio Inc, O=New Owner"),
                Some(&names),
                false
            ),
            PublisherDecision::Accept
        );
        let dn_names = ["CN=Studio Inc, C=PT".to_owned()];
        assert_eq!(
            decide_publisher(
                &own,
                &signed("CN=Studio Inc, O=X, C=PT"),
                Some(&dn_names),
                false
            ),
            PublisherDecision::Accept
        );
        let no_subject = Authenticode {
            subject: None,
            ..own.clone()
        };
        assert!(matches!(
            decide_publisher(&no_subject, &own, None, false),
            PublisherDecision::Reject(_)
        ));
        assert!(matches!(
            decide_publisher(&own, &no_subject, None, false),
            PublisherDecision::Reject(_)
        ));
        // Configured names are enforced also when the running app is
        // unsigned (electron-updater reads publisherName, not the app).
        let unsigned = Authenticode {
            status: 2,
            subject: None,
            ..own.clone()
        };
        assert!(matches!(
            decide_publisher(&unsigned, &unsigned, Some(&names), false),
            PublisherDecision::Reject(_)
        ));
        assert_eq!(
            decide_publisher(&unsigned, &own, Some(&names), true),
            PublisherDecision::Accept
        );
        // Overwolf's certificate on the app exe: no default publisher.
        assert_eq!(
            decide_publisher(&signed("CN=Overwolf"), &unsigned, None, true),
            PublisherDecision::SkipNoPublisher
        );
        assert!(parse_authenticode("{}").is_err());
        assert!(parse_authenticode("").is_err());
        let r = parse_authenticode("\u{feff}{\"Status\":2,\"SignerCertificate\":null}").unwrap();
        assert!(!r.is_valid() && r.subject.is_none());
    }
}
