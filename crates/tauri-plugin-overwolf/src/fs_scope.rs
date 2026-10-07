//! The file-system scope of the `fs_*` commands and of `shell_open_path`
//! (CONTRACT A.2.3, A.2.3.2).
//!
//! | Root | Access |
//! |---|---|
//! | `paths.userData` and below | read-write |
//! | the per-app state directory (F.1) | read-only |
//! | `<appPath>/package.json` | read-only, served from the embedded manifest |
//! | each `fs.scope` template | read-write |
//!
//! Paths are canonicalised before the check: symlinks are resolved for the
//! part of the path that exists, and the rest may not contain `..`. A path
//! that leaves every root is `forbidden`.
//!
//! ```
//! use std::path::Path;
//! use tauri_plugin_overwolf::fs_scope::{Access, FsScope, Resolved};
//! let root = std::env::temp_dir().join(format!("fs-scope-doc-{}", std::process::id()));
//! std::fs::create_dir_all(root.join("user")).unwrap();
//! let scope = FsScope::new(root.join("user"), root.join("state"), Path::new("/virtual/app"), Vec::new());
//! assert!(matches!(scope.check(&root.join("user/prefs.json"), Access::Write), Ok(Resolved::File(_))));
//! assert!(scope.check(&root.join("user/../escape.txt"), Access::Read).is_err());
//! assert!(matches!(scope.check(std::path::Path::new("/virtual/app/package.json"), Access::Read), Ok(Resolved::Manifest)));
//! # let _ = std::fs::remove_dir_all(&root);
//! ```

use std::path::{Component, Path, PathBuf};

use crate::error::Error;

/// What a caller wants to do with a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Read or test for existence.
    Read,
    /// Create, write or make directories.
    Write,
}

/// Where an allowed path points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    /// A real file-system path (canonical form).
    File(PathBuf),
    /// `<appPath>/package.json`, served from the embedded manifest.
    Manifest,
}

/// The base directories an `fs.scope` template may start with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TemplateDirs {
    /// `$USERDATA`.
    pub user_data: PathBuf,
    /// `$PICTURES`.
    pub pictures: PathBuf,
    /// `$VIDEOS`.
    pub videos: PathBuf,
    /// `$DOCUMENTS`.
    pub documents: PathBuf,
    /// `$DOWNLOADS`.
    pub downloads: PathBuf,
    /// `$TEMP`.
    pub temp: PathBuf,
    /// `$APPNAME` (`productName`).
    pub app_name: String,
}

/// Expands one validated `fs.scope` template. Returns `None` when its base
/// directory is unknown on this system.
///
/// ```
/// use std::path::PathBuf;
/// use tauri_plugin_overwolf::fs_scope::{expand_template, TemplateDirs};
/// let dirs = TemplateDirs { pictures: PathBuf::from("/p"), app_name: "Example App".into(), ..TemplateDirs::default() };
/// assert_eq!(expand_template("$PICTURES/Overwolf/$APPNAME", &dirs), Some(PathBuf::from("/p/Overwolf/Example App")));
/// assert_eq!(expand_template("$VIDEOS/x", &dirs), None);
/// ```
#[must_use]
pub fn expand_template(template: &str, dirs: &TemplateDirs) -> Option<PathBuf> {
    let table: [(&str, &Path); 6] = [
        ("$USERDATA", &dirs.user_data),
        ("$PICTURES", &dirs.pictures),
        ("$VIDEOS", &dirs.videos),
        ("$DOCUMENTS", &dirs.documents),
        ("$DOWNLOADS", &dirs.downloads),
        ("$TEMP", &dirs.temp),
    ];
    let replaced = template.replace("$APPNAME", &dirs.app_name);
    for (var, base) in table {
        if let Some(rest) = replaced.strip_prefix(var)
            && (rest.is_empty() || rest.starts_with('/') || rest.starts_with('\\'))
        {
            if base.as_os_str().is_empty() {
                return None;
            }
            let rest = rest.trim_start_matches(['/', '\\']);
            return Some(if rest.is_empty() {
                base.to_path_buf()
            } else {
                base.join(rest)
            });
        }
    }
    let path = PathBuf::from(replaced);
    path.is_absolute().then_some(path)
}

/// Canonicalises `path`: resolves symlinks in the longest existing prefix and
/// appends the remaining components, which must be plain names.
///
/// # Errors
///
/// `forbidden` for a relative path or a `..` / `.` in the part that does not
/// exist yet; `io` when the existing part cannot be resolved.
///
/// ```
/// use std::path::Path;
/// use tauri_plugin_overwolf::fs_scope::canonicalize_lenient;
/// let tmp = std::env::temp_dir();
/// let p = canonicalize_lenient(&tmp.join("ow-tauri-doc-missing").join("file.txt")).unwrap();
/// assert!(p.ends_with("ow-tauri-doc-missing/file.txt"));
/// assert!(canonicalize_lenient(Path::new("relative/file.txt")).is_err());
/// assert!(canonicalize_lenient(&tmp.join("ow-tauri-doc-missing").join("..").join("x")).is_err());
/// ```
pub fn canonicalize_lenient(path: &Path) -> Result<PathBuf, Error> {
    if !path.is_absolute() {
        return Err(Error::forbidden("path must be absolute"));
    }
    // Lexically drop `.` and resolve `..` only against existing ancestors:
    // walk up until an ancestor exists, then canonicalise it.
    let mut existing = path.to_path_buf();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    loop {
        match std::fs::canonicalize(&existing) {
            Ok(canon) => {
                let mut out = canon;
                for name in tail.iter().rev() {
                    out.push(name);
                }
                return Ok(out);
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let Some(name) = existing.file_name().map(std::ffi::OsStr::to_os_string) else {
                    return Err(Error::forbidden("path is outside the allowed scope"));
                };
                let last = existing.components().next_back();
                if !matches!(last, Some(Component::Normal(_))) {
                    return Err(Error::forbidden("path is outside the allowed scope"));
                }
                tail.push(name);
                if !existing.pop() {
                    return Err(Error::forbidden("path is outside the allowed scope"));
                }
            }
            Err(err) => return Err(Error::from_io("Resolving the path", &err)),
        }
    }
}

fn has_dot_segments(path: &Path) -> bool {
    path.components()
        .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Root {
    path: PathBuf,
    write: bool,
}

/// The `fs_*` scope for one app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsScope {
    roots: Vec<Root>,
    manifest_path: PathBuf,
}

impl FsScope {
    /// Builds the scope from `userData` (read-write), the state directory
    /// (read-only), the virtual app root and the expanded `fs.scope`
    /// directories (read-write).
    ///
    /// ```
    /// use tauri_plugin_overwolf::fs_scope::{Access, FsScope};
    /// let tmp = std::env::temp_dir();
    /// let scope = FsScope::new(tmp.join("user-data"), tmp.join("state"), &tmp.join("app"), Vec::new());
    /// assert!(scope.check(&tmp.join("user-data").join("prefs.json"), Access::Write).is_ok());
    /// assert!(scope.check(&tmp.join("state").join("log.txt"), Access::Write).is_err());
    /// ```
    #[must_use]
    pub fn new(
        user_data: PathBuf,
        state_dir: PathBuf,
        app_path: &Path,
        extra_rw: Vec<PathBuf>,
    ) -> Self {
        let mut roots = vec![
            Root {
                path: user_data,
                write: true,
            },
            Root {
                path: state_dir,
                write: false,
            },
        ];
        roots.extend(extra_rw.into_iter().map(|path| Root { path, write: true }));
        let roots = roots
            .into_iter()
            .filter(|r| r.path.is_absolute())
            .map(|r| Root {
                path: canonicalize_lenient(&r.path).unwrap_or(r.path),
                write: r.write,
            })
            .collect();
        FsScope {
            roots,
            manifest_path: app_path.join("package.json"),
        }
    }

    /// The virtual `package.json` path.
    ///
    /// ```
    /// use tauri_plugin_overwolf::fs_scope::FsScope;
    /// let tmp = std::env::temp_dir();
    /// let scope = FsScope::new(tmp.join("u"), tmp.join("s"), &tmp.join("app"), Vec::new());
    /// assert_eq!(scope.manifest_path(), tmp.join("app").join("package.json"));
    /// ```
    #[must_use]
    pub fn manifest_path(&self) -> &Path {
        &self.manifest_path
    }

    /// Checks `path` for `access`.
    ///
    /// # Errors
    ///
    /// `forbidden` when the path is relative, contains `..` that leaves a
    /// root, or is outside every root with that access; `io` when it cannot
    /// be resolved.
    ///
    /// ```
    /// use tauri_plugin_overwolf::fs_scope::{Access, FsScope, Resolved};
    /// let tmp = std::env::temp_dir();
    /// let scope = FsScope::new(tmp.join("u"), tmp.join("s"), &tmp.join("app"), Vec::new());
    /// assert_eq!(scope.check(scope.manifest_path(), Access::Read).unwrap(), Resolved::Manifest);
    /// assert!(scope.check(scope.manifest_path(), Access::Write).is_err());
    /// let escape = tmp.join("u").join("..").join("elsewhere.txt");
    /// assert_eq!(scope.check(&escape, Access::Read).unwrap_err().code().as_str(), "forbidden");
    /// ```
    pub fn check(&self, path: &Path, access: Access) -> Result<Resolved, Error> {
        if path == self.manifest_path {
            return if access == Access::Read {
                Ok(Resolved::Manifest)
            } else {
                Err(Error::forbidden("the app manifest is read-only"))
            };
        }
        if !path.is_absolute() {
            return Err(Error::forbidden("path must be absolute"));
        }
        let canonical = canonicalize_lenient(path)?;
        if has_dot_segments(&canonical) {
            return Err(Error::forbidden("path is outside the allowed scope"));
        }
        let allowed = self.roots.iter().any(|root| {
            canonical.starts_with(&root.path) && (access == Access::Read || root.write)
        });
        if allowed {
            Ok(Resolved::File(canonical))
        } else if self
            .roots
            .iter()
            .any(|root| canonical.starts_with(&root.path))
        {
            Err(Error::forbidden("path is read-only"))
        } else {
            Err(Error::forbidden("path is outside the allowed scope"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use crate::state::test_dir;

    fn scope(root: &Path) -> FsScope {
        std::fs::create_dir_all(root.join("user")).unwrap();
        std::fs::create_dir_all(root.join("state")).unwrap();
        std::fs::create_dir_all(root.join("pics")).unwrap();
        std::fs::create_dir_all(root.join("outside")).unwrap();
        FsScope::new(
            root.join("user"),
            root.join("state"),
            Path::new("/virtual/app"),
            vec![root.join("pics/Overwolf/App")],
        )
    }

    #[test]
    fn read_write_and_read_only_roots() {
        let dir = test_dir("fs-scope");
        let s = scope(&dir);
        assert!(s.check(&dir.join("user/a/b.json"), Access::Write).is_ok());
        assert!(
            s.check(&dir.join("state/ow-tauri.json"), Access::Read)
                .is_ok()
        );
        let err = s
            .check(&dir.join("state/ow-tauri.json"), Access::Write)
            .unwrap_err();
        assert_eq!(err.code(), ErrorCode::Forbidden);
        assert!(
            s.check(&dir.join("pics/Overwolf/App/shot.png"), Access::Write)
                .is_ok()
        );
        assert!(s.check(&dir.join("pics/other.png"), Access::Read).is_err());
        assert!(s.check(&dir.join("outside/x"), Access::Read).is_err());
        assert!(s.check(Path::new("relative/x"), Access::Read).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dot_dot_cannot_escape() {
        let dir = test_dir("fs-dotdot");
        let s = scope(&dir);
        // Existing prefix resolves `..` canonically: user/../outside is outside.
        assert!(
            s.check(&dir.join("user/../outside/x"), Access::Read)
                .is_err()
        );
        // `..` inside the non-existing tail is refused outright.
        assert!(
            s.check(&dir.join("user/missing/../../outside/x"), Access::Read)
                .is_err()
        );
        // `..` that stays inside is fine when the prefix exists.
        std::fs::create_dir_all(dir.join("user/sub")).unwrap();
        assert!(
            s.check(&dir.join("user/sub/../ok.txt"), Access::Write)
                .is_ok()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_out_of_scope_is_forbidden() {
        let dir = test_dir("fs-symlink");
        let s = scope(&dir);
        std::fs::write(dir.join("outside/secret"), b"x").unwrap();
        std::os::unix::fs::symlink(dir.join("outside"), dir.join("user/link")).unwrap();
        assert!(
            s.check(&dir.join("user/link/secret"), Access::Read)
                .is_err()
        );
        assert!(s.check(&dir.join("user/link/new"), Access::Write).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn manifest_is_virtual_and_read_only() {
        let dir = test_dir("fs-manifest");
        let s = scope(&dir);
        let p = Path::new("/virtual/app/package.json");
        assert_eq!(s.check(p, Access::Read).unwrap(), Resolved::Manifest);
        assert!(s.check(p, Access::Write).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn templates() {
        let dirs = TemplateDirs {
            user_data: PathBuf::from("/u"),
            temp: PathBuf::from("/t"),
            app_name: "App".into(),
            ..TemplateDirs::default()
        };
        assert_eq!(
            expand_template("$USERDATA", &dirs),
            Some(PathBuf::from("/u"))
        );
        assert_eq!(
            expand_template("$TEMP/$APPNAME/x", &dirs),
            Some(PathBuf::from("/t/App/x"))
        );
        assert_eq!(expand_template("$USERDATAX", &dirs), None);
        // An absolute path needs a drive or UNC prefix on Windows.
        let (abs, expanded) = if cfg!(windows) {
            (r"C:\abs\$APPNAME", r"C:\abs\App")
        } else {
            ("/abs/$APPNAME", "/abs/App")
        };
        assert_eq!(expand_template(abs, &dirs), Some(PathBuf::from(expanded)));
        assert_eq!(
            expand_template("/abs/$APPNAME", &dirs).is_some(),
            cfg!(not(windows))
        );
        assert_eq!(expand_template("rel", &dirs), None);
    }
}
