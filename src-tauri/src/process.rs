use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::Command;

use crate::errors::AppError;

/// `CREATE_NO_WINDOW`. Stops a console flashing when a GUI app starts a child.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Start a child and return as soon as the OS has accepted the spawn.
/// The child is not waited on and no thread is kept. A waiter would outlive
/// `app.exit` and leave this exe running after the window closed.
pub fn spawn_detached(command: &mut Command) -> std::io::Result<()> {
    let _child = command.spawn()?;
    Ok(())
}

/// Every process this app starts goes through here.
/// On Windows the child is created with [`CREATE_NO_WINDOW`].
pub fn command(program: impl AsRef<OsStr>) -> Command {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut command = Command::new(program);
        command.creation_flags(CREATE_NO_WINDOW);
        command
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new(program)
    }
}

/// `explorer /select,<path>` — one argument, comma, no space.
pub fn explorer_select_arg(path: &Path) -> OsString {
    let mut arg = OsString::from("/select,");
    arg.push(path.as_os_str());
    arg
}

pub fn reveal_path(path: &Path) -> Result<(), AppError> {
    if path.as_os_str().is_empty() || !path.exists() {
        return Err(reveal_error(&format!(
            "file is missing: {}",
            path.display()
        )));
    }
    #[cfg(target_os = "windows")]
    {
        spawn_detached(command("explorer").arg(explorer_select_arg(path)))
            .map_err(|err| reveal_error(&err.to_string()))?;
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        spawn_detached(command("open").arg("-R").arg(path))
            .map_err(|err| reveal_error(&err.to_string()))?;
        return Ok(());
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let dir = path
            .parent()
            .filter(|dir| !dir.as_os_str().is_empty())
            .unwrap_or(path);
        spawn_detached(command("xdg-open").arg(dir))
            .map_err(|err| reveal_error(&err.to_string()))?;
        Ok(())
    }
}

fn reveal_error(detail: &str) -> AppError {
    AppError::Other {
        code: "E-REVEAL".into(),
        title: "The log folder could not be opened".into(),
        hint: "The file was still saved. Copy the path and open it yourself.".into(),
        detail: detail.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_no_window_is_the_win32_flag() {
        assert_eq!(CREATE_NO_WINDOW, 0x0800_0000);
    }

    #[test]
    fn release_binary_uses_the_windows_subsystem() {
        let source = include_str!("main.rs");
        assert!(
            source.contains("#![cfg_attr(not(debug_assertions), windows_subsystem = \"windows\")]"),
            "release builds must hide the console"
        );
    }

    #[test]
    fn detached_spawn_does_not_keep_a_waiter_thread() {
        let source = include_str!("process.rs");
        let body = source
            .split("pub fn spawn_detached")
            .nth(1)
            .expect("spawn_detached")
            .split("pub fn command")
            .next()
            .expect("command");
        assert!(
            !body.contains("thread::spawn"),
            "a waiter thread would keep the installer alive after exit"
        );
    }

    #[test]
    fn explorer_select_uses_the_comma_form() {
        let arg = explorer_select_arg(Path::new(
            r"C:\Users\Sam\Documents\Arnold Installer\logs\Arnold-install-log-2026-11-02-1915.txt",
        ));
        let text = arg.to_string_lossy();
        assert!(text.starts_with("/select,"));
        assert!(text.contains("Arnold-install-log-2026-11-02-1915.txt"));
        assert!(!text.contains("/select "));
    }
}
