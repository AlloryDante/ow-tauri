//! The update client's OS layer ([`UpdateOs`]): Windows runs the
//! Authenticode check through PowerShell and starts the installer; every
//! other OS answers `unsupported` (R6).

use std::sync::Arc;

use super::engine::UpdateOs;

/// The OS layer of this build.
pub(crate) fn system() -> Arc<dyn UpdateOs> {
    #[cfg(windows)]
    {
        Arc::new(windows::WindowsOs)
    }
    #[cfg(not(windows))]
    {
        Arc::new(Unsupported)
    }
}

/// macOS, Linux and mobile: the client does not run (R6).
#[cfg(not(windows))]
pub(crate) struct Unsupported;

#[cfg(not(windows))]
impl Unsupported {
    fn error() -> crate::Error {
        crate::Error::unsupported(
            "the Overwolf update client runs on Windows only; use tauri-plugin-updater on this OS",
        )
    }
}

#[cfg(not(windows))]
impl UpdateOs for Unsupported {
    fn supported(&self) -> crate::Result<()> {
        Err(Self::error())
    }

    fn os_release(&self) -> String {
        String::new()
    }

    fn authenticode(&self, _file: &std::path::Path) -> crate::Result<super::verify::Authenticode> {
        Err(Self::error())
    }

    fn launch(&self, _plan: &super::install::WindowsInstall) -> crate::Result<()> {
        Err(Self::error())
    }
}

#[cfg(windows)]
mod windows {
    use std::path::{Path, PathBuf};

    use super::super::engine::UpdateOs;
    use super::super::install::{WindowsInstall, quote_windows_arg};
    use super::super::verify::{self, Authenticode};
    use crate::error::{Error, Result};

    /// The Windows OS layer.
    pub(crate) struct WindowsOs;

    impl UpdateOs for WindowsOs {
        fn supported(&self) -> Result<()> {
            Ok(())
        }

        fn os_release(&self) -> String {
            crate::platform::os_release()
        }

        fn authenticode(&self, file: &Path) -> Result<Authenticode> {
            authenticode(file)
        }

        fn launch(&self, plan: &WindowsInstall) -> Result<()> {
            launch(plan)
        }
    }

    /// The Windows system folder (`GetSystemDirectoryW`), where
    /// `powershell.exe`'s folder lives. Programs start by full path from
    /// it, never through the search order, which looks in the
    /// (user-writable) app folder first.
    fn system32() -> PathBuf {
        use std::os::windows::ffi::OsStringExt as _;
        let mut buf = vec![0_u16; 512];
        // SAFETY: the buffer is valid for `len` UTF-16 units.
        let n = unsafe {
            windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW(
                buf.as_mut_ptr(),
                u32::try_from(buf.len()).unwrap_or(0),
            )
        };
        let n = usize::try_from(n).unwrap_or(0);
        if n == 0 || n >= buf.len() {
            return PathBuf::from(r"C:\Windows\System32");
        }
        PathBuf::from(std::ffi::OsString::from_wide(&buf[..n]))
    }

    /// `Get-AuthenticodeSignature` through PowerShell, as electron-updater
    /// runs it. The path reaches the script through an environment
    /// variable, so no character of it is ever parsed as script. Any
    /// failure (PowerShell missing or blocked, no report) is a
    /// `verification` error.
    fn authenticode(file: &Path) -> Result<Authenticode> {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const SCRIPT: &str = "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; Get-AuthenticodeSignature -LiteralPath $env:OW_TAURI_UPDATE_FILE | ConvertTo-Json -Compress";
        let powershell = system32()
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");
        let out = std::process::Command::new(powershell)
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-InputFormat",
                "None",
                "-Command",
                SCRIPT,
            ])
            .env("OW_TAURI_UPDATE_FILE", file)
            .env("PSModulePath", "")
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|_| Error::verification("The signature check could not run."))?;
        verify::parse_authenticode(&String::from_utf8_lossy(&out.stdout))
    }

    fn launch(plan: &WindowsInstall) -> Result<()> {
        use std::os::windows::ffi::OsStrExt as _;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        if !plan.elevate {
            std::process::Command::new(&plan.program)
                .args(&plan.args)
                .spawn()
                .map_err(|e| Error::from_io("Starting the update installer", &e))?;
            return Ok(());
        }
        let wide = |s: &std::ffi::OsStr| {
            s.encode_wide()
                .chain(std::iter::once(0))
                .collect::<Vec<u16>>()
        };
        let params = plan
            .args
            .iter()
            .map(|a| quote_windows_arg(a))
            .collect::<Vec<_>>()
            .join(" ");
        let verb = wide(std::ffi::OsStr::new("runas"));
        let file = wide(plan.program.as_os_str());
        let params = wide(std::ffi::OsStr::new(&params));
        // SAFETY: every pointer is a NUL-terminated UTF-16 buffer that
        // outlives the call; a null window handle and directory are allowed.
        let code = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                params.as_ptr(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        } as isize;
        if code > 32 {
            Ok(())
        } else {
            Err(Error::from_io(
                "Starting the elevated update installer",
                &std::io::Error::other("ShellExecuteW failed"),
            ))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// §7.4: an unsigned file fails the publisher check through the
        /// real PowerShell report.
        #[test]
        fn unsigned_file_fails_closed() {
            let dir = crate::state::test_dir("updater-authenticode");
            let file = dir.join("setup.exe");
            std::fs::write(&file, b"MZ not really a program").unwrap();
            let names = ["Example Studio".to_owned()];
            let result =
                authenticode(&file).and_then(|r| verify::check_publisher(&r, &file, &names));
            assert_eq!(result.unwrap_err().code(), crate::ErrorCode::Verification);
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}
