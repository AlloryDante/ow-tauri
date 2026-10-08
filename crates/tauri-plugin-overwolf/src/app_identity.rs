//! The app's Overwolf identity from the Tauri configuration (DESIGN §3.1):
//! `<PN>`, version, author, uid and cuid.
//!
//! The build step and the plugin resolve it from the same merged
//! configuration with [`AppIdentity::resolve`], so the uid in the installer's
//! records always equals the uid at run time.

use crate::config::Config;
use crate::identity::computed_uid;

/// The author the uid formula uses when `plugins.overwolf.author` is missing
/// or empty (CONTRACT G.2); only debug builds accept it without a pinned
/// uid.
pub(crate) const UNKNOWN_AUTHOR: &str = "unknown";

/// The app's identity (DESIGN §3.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AppIdentity {
    /// `<PN>`: `plugins.overwolf.name`, else the Tauri `productName`, else
    /// the Cargo package name (as Tauri falls back).
    pub(crate) name: String,
    /// The app version.
    pub(crate) version: String,
    /// The uid formula's author (`"unknown"` when not configured).
    pub(crate) author: String,
    /// The effective uid: the trimmed `plugins.overwolf.uid`, else the cuid.
    pub(crate) uid: String,
    /// The computed uid, always the formula of `author` and `name`.
    pub(crate) cuid: String,
}

impl AppIdentity {
    /// Resolves the identity. `product_name` is Tauri's resolved product
    /// name (`productName`, else the Cargo package name: Tauri's
    /// `PackageInfo::name` at run time), `version` the app version.
    ///
    /// The uid must already have passed [`Config::validate`]; an invalid
    /// configured uid is ignored here (the formula applies), so the state
    /// directory never gets a path separator from it.
    pub(crate) fn resolve(config: &Config, product_name: &str, version: &str) -> Self {
        let name = config
            .name
            .clone()
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| product_name.to_owned());
        let author = config
            .author
            .clone()
            .filter(|a| !a.is_empty())
            .unwrap_or_else(|| UNKNOWN_AUTHOR.to_owned());
        let cuid = computed_uid(&author, &name);
        let uid = config
            .uid
            .as_deref()
            .map(str::trim)
            .filter(|u| crate::identity::is_valid_uid(u))
            .map_or_else(|| cuid.clone(), str::to_owned);
        AppIdentity {
            name,
            version: version.to_owned(),
            author,
            uid,
            cuid,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(raw: &serde_json::Value) -> Config {
        Config::from_value(raw).unwrap()
    }

    /// CONTRACT G.2 vectors 1 to 12 as (author, `<PN>`, uid): the values an
    /// ow-electron `package.json` resolves to, which `ow-tauri migrate`
    /// writes as `plugins.overwolf.author` and `name`.
    #[test]
    fn uid_vectors() {
        let vectors = [
            (
                "Example Studio",
                "parity-harness",
                "binaioonkjpolnojeenpbmjmbfkbmffcekndbmdk",
            ),
            (
                "Example Studio",
                "Parity Harness",
                "bijigndkghcikkfmhgkmicdkjpdehpjafgpmdhcc",
            ),
            (
                "Overwolf Ltd.",
                "parity-harness",
                "djaoacjhpjaenfddlfmkeoiklmccgcgcgeknhmgj",
            ),
            (
                "Overwolf Ltd.",
                "Parity Harness",
                "aejkligdodglhcjinbhdcnlohocenfkpdihjacdg",
            ),
            (
                "Example Studio <dev@example.com> (https://example.com)",
                "parity-harness",
                "agmekflfehlhfcnofnhghbgohnigngnkddpkdbnc",
            ),
            (
                "Example Studio <dev@example.com> (https://example.com)",
                "Parity Harness",
                "cmbaaahkhdbkbbfcmenfbmmngmommpjjacllbgan",
            ),
            (
                "Example Studio <dev@example.com>",
                "parity-harness",
                "mcfopdapolegaeddgbbfedginnmcnmjldgdcbcjo",
            ),
            (
                "",
                "parity-harness",
                "nbhlaphlggihmjefpjdelbobckfhklbfkiicjaja",
            ),
            (
                "",
                "Parity Harness",
                "fifpcfmoobnjlimjhefehejankadpajlfgbmpheo",
            ),
            (
                "Exämple",
                "Pârity Ünicode",
                "mmfoflmmchoacblhjlimpanaijdnhgoalaloihjd",
            ),
            (
                "D'Arcy",
                "O'Brien Tools",
                "khalfglcmeemfnjoldckbfmeeidgkoabeebkpbbl",
            ),
            (
                " Example Studio ",
                " Parity Harness ",
                "cppaiialckdbmhdojecejpjafcblbingfdiffkdi",
            ),
        ];
        for (author, name, uid) in vectors {
            let id = AppIdentity::resolve(
                &config(&serde_json::json!({ "author": author, "name": name })),
                "Ignored Product Name",
                "1.0.0",
            );
            assert_eq!(id.uid, uid, "{author:?} {name:?}");
            assert_eq!(id.cuid, uid);
            assert_eq!(id.name, name);
        }
    }

    #[test]
    fn name_falls_back_to_the_product_name() {
        let id = AppIdentity::resolve(&Config::default(), "Parity Harness", "2.0.0");
        assert_eq!(id.name, "Parity Harness");
        assert_eq!(id.author, UNKNOWN_AUTHOR);
        assert_eq!(id.version, "2.0.0");
        assert_eq!(id.uid, "fifpcfmoobnjlimjhefehejankadpajlfgbmpheo");
    }

    /// Vector 13: a configured uid wins after trimming; the cuid stays the
    /// formula.
    #[test]
    fn configured_uid_wins() {
        let c = config(&serde_json::json!({
            "author": "Example Studio",
            "name": "parity-harness",
            "uid": " aaaabbbbccccddddeeeeffffgggghhhhiiiijjjj "
        }));
        let id = AppIdentity::resolve(&c, "Display Name", "1.0.0");
        assert_eq!(id.uid, "aaaabbbbccccddddeeeeffffgggghhhhiiiijjjj");
        assert_eq!(id.cuid, "binaioonkjpolnojeenpbmjmbfkbmffcekndbmdk");
    }

    #[test]
    fn an_invalid_uid_never_names_the_state_directory() {
        let c = config(&serde_json::json!({ "author": "A", "name": "N", "uid": "../escape" }));
        let id = AppIdentity::resolve(&c, "P", "1");
        assert_eq!(id.uid, id.cuid);
    }
}
