//! Download verification (CONTRACT I.3, DESIGN §4.14 [R5],
//! [ADR 0008](https://github.com/AlloryDante/ow-tauri/blob/main/docs/adr/0008-updater-client.md)):
//! the SHA-512 of the feed entry, the detached minisign signature, and the
//! parts of the Windows publisher check that need no OS: the Authenticode
//! report of PowerShell's `Get-AuthenticodeSignature` and electron-updater's
//! publisher-name rule.
//!
//! Every check fails closed with a `verification` error: anything
//! unreadable is a failure. Nothing is trusted by default (R5): the
//! installer must match `updater.publisherNames` or verify against
//! `updater.pubkey`.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Read;
use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use sha2::{Digest, Sha512};

use crate::error::Error;

fn failed(message: &str) -> Error {
    Error::verification(message.to_owned())
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
    let file =
        std::fs::File::open(path).map_err(|e| Error::from_io("Reading the update file", &e))?;
    sha512_reader(file)
}

/// SHA-512 of everything `reader` yields (the locked installer handle).
///
/// # Errors
///
/// `io` when a read fails.
pub fn sha512_reader(mut file: impl Read) -> Result<Vec<u8>, Error> {
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
/// `verification` when the signature does not parse or does not verify,
/// `io` when the file cannot be read.
pub fn verify_minisign(
    pubkey: &minisign_verify::PublicKey,
    signature: &str,
    path: &Path,
) -> Result<(), Error> {
    let file =
        std::fs::File::open(path).map_err(|e| Error::from_io("Reading the update file", &e))?;
    verify_minisign_reader(pubkey, signature, file)
}

/// [`verify_minisign`] over everything `file` yields (the locked installer
/// handle).
///
/// # Errors
///
/// As [`verify_minisign`].
pub fn verify_minisign_reader(
    pubkey: &minisign_verify::PublicKey,
    signature: &str,
    mut file: impl Read,
) -> Result<(), Error> {
    let sig = minisign_verify::Signature::decode(&unwrap_base64_text(signature))
        .map_err(|_| failed("The update signature file does not parse."))?;
    let mut verifier = pubkey
        .verify_stream(&sig)
        .map_err(|_| failed("The update signature was made with another key."))?;
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
        .map_err(|_| failed("The update signature does not match the file."))
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
        serde_json::from_str(t).map_err(|_| failed("The signature check gave no report."))?;
    let status = v
        .get("Status")
        .and_then(serde_json::Value::as_i64)
        .ok_or_else(|| failed("The signature check report has no status."))?;
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

/// The Windows publisher check (R5): the installer carries a valid
/// Authenticode signature whose subject matches one of `publisher_names`
/// ([`publisher_matches`]), and the report is about `file` (PowerShell
/// checked the file the client downloaded).
///
/// # Errors
///
/// `verification` naming the failed condition.
///
/// ```
/// use std::path::Path;
/// use tauri_plugin_overwolf::updater::verify::{check_publisher, Authenticode};
/// let report = |status: i64, s: &str| Authenticode {
///     status,
///     status_message: String::new(),
///     subject: Some(s.into()),
///     path: r"C:\Temp\setup.exe".into(),
/// };
/// let file = Path::new(r"C:\Temp\setup.exe");
/// let names = ["Studio".to_owned()];
/// assert!(check_publisher(&report(0, "CN=Studio"), file, &names).is_ok());
/// assert!(check_publisher(&report(0, "CN=Other"), file, &names).is_err());
/// assert!(check_publisher(&report(2, "CN=Studio"), file, &names).is_err());
/// assert!(check_publisher(&report(0, "CN=Studio"), Path::new(r"C:\x.exe"), &names).is_err());
/// ```
pub fn check_publisher(
    report: &Authenticode,
    file: &Path,
    publisher_names: &[String],
) -> Result<(), Error> {
    if !same_windows_path(&report.path, &file.to_string_lossy()) {
        return Err(failed("The signature check read another file."));
    }
    if !report.is_valid() {
        return Err(failed("The update installer has no valid signature."));
    }
    let Some(subject) = report.subject.as_deref() else {
        return Err(failed("The update installer's signer is unknown."));
    };
    if publisher_matches(subject, publisher_names) {
        Ok(())
    } else {
        Err(failed(
            "The update installer is signed by another publisher.",
        ))
    }
}

/// Whether two Windows paths name the same file: `/` and `\\` alike, no
/// trailing separator, case-insensitive (NTFS default).
///
/// ```
/// use tauri_plugin_overwolf::updater::verify::same_windows_path;
/// assert!(same_windows_path(r"C:\A\setup.exe", "c:/a/SETUP.exe"));
/// assert!(!same_windows_path(r"C:\A\setup.exe", r"C:\A\setup.exe.bak"));
/// ```
#[must_use]
pub fn same_windows_path(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.replace('/', "\\").trim_end_matches('\\').to_lowercase();
    !a.is_empty() && norm(a) == norm(b)
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
            crate::ErrorCode::Verification
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
    fn publisher_check_fails_closed() {
        let file = Path::new(r"C:\Users\Public\pending\setup.exe");
        let report = |status: i64, subject: Option<&str>, path: &str| Authenticode {
            status,
            status_message: String::new(),
            subject: subject.map(str::to_owned),
            path: path.to_owned(),
        };
        let here = file.to_string_lossy().into_owned();
        let names = ["Studio Inc".to_owned()];
        let dn_names = ["CN=Studio Inc, C=PT".to_owned()];
        let ok = report(0, Some("CN=Studio Inc, O=Studio Inc, C=PT"), &here);
        check_publisher(&ok, file, &names).unwrap();
        check_publisher(&ok, file, &dn_names).unwrap();
        let code = |r: &Authenticode, names: &[String]| {
            check_publisher(r, file, names).unwrap_err().code()
        };
        let verification = crate::ErrorCode::Verification;
        // No names: nothing is trusted (R5).
        assert_eq!(code(&ok, &[]), verification);
        assert_eq!(
            code(&report(0, Some("CN=Other"), &here), &names),
            verification
        );
        assert_eq!(code(&report(0, None, &here), &names), verification);
        assert_eq!(
            code(&report(2, Some("CN=Studio Inc"), &here), &names),
            verification
        );
        assert_eq!(
            code(&report(0, Some("CN=Studio Inc"), r"C:\other.exe"), &names),
            verification
        );
        assert_eq!(
            code(&report(0, Some("CN=Studio Inc"), ""), &names),
            verification
        );
        assert!(parse_authenticode("{}").is_err());
        assert!(parse_authenticode("").is_err());
        let r = parse_authenticode("\u{feff}{\"Status\":2,\"SignerCertificate\":null}").unwrap();
        assert!(!r.is_valid() && r.subject.is_none());
    }

    #[test]
    fn reader_forms_match_the_file_forms() {
        let file = temp_file("reader", b"test");
        let pk = parse_public_key(KEY).unwrap();
        verify_minisign_reader(&pk, SIG, std::fs::File::open(&file).unwrap()).unwrap();
        assert!(verify_minisign_reader(&pk, SIG, &b"Test"[..]).is_err());
        assert_eq!(
            sha512_reader(&b"test"[..]).unwrap(),
            sha512_file(&file).unwrap()
        );
    }
}
