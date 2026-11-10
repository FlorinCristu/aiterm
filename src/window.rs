//! New tab / new window, opened in the current folder.
//!
//! aiterm is a shell front-end living inside a host terminal, so a "tab" is a tab of that host
//! (Windows Terminal) and falls back to a new window everywhere else.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

/// Folder the shell is in right now; the global shortcut hook reads it from another thread.
static CWD: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn set_cwd(dir: &Path) {
    if let Ok(mut c) = CWD.lock() {
        *c = Some(dir.to_path_buf());
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
pub fn current_cwd() -> Option<PathBuf> {
    CWD.lock().ok().and_then(|c| c.clone())
}

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Tab,
    Window,
}

/// Open another aiterm in `dir`. Returns what happened, for the user.
pub fn open(kind: Kind, dir: &Path) -> Result<&'static str, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    spawn(kind, dir, &exe)
}

#[cfg(windows)]
fn spawn(kind: Kind, dir: &Path, exe: &Path) -> Result<&'static str, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x10;
    if std::env::var_os("WT_SESSION").is_some() {
        let target = if kind == Kind::Tab { "0" } else { "-1" }; // 0 = this window, -1 = a new one
        let status = Command::new("wt")
            .args(["-w", target, "new-tab", "-d"])
            .arg(dir)
            .arg(exe)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if status.map(|s| s.success()).unwrap_or(false) {
            return Ok(if kind == Kind::Tab { "new tab" } else { "new window" });
        }
    }
    Command::new(exe)
        .current_dir(dir)
        .creation_flags(CREATE_NEW_CONSOLE)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok("new window")
}

#[cfg(not(windows))]
fn spawn(_: Kind, dir: &Path, exe: &Path) -> Result<&'static str, String> {
    if cfg!(target_os = "macos") {
        let script = format!(
            "tell application \"Terminal\" to do script \"cd '{}' && exec '{}'\"",
            dir.display().to_string().replace('\'', "'\\''"),
            exe.display().to_string().replace('\'', "'\\''")
        );
        Command::new("osascript").args(["-e", &script]).stdout(Stdio::null()).spawn().map_err(|e| e.to_string())?;
        return Ok("new window");
    }
    let mut candidates: Vec<String> = std::env::var("TERMINAL").ok().into_iter().collect();
    candidates.extend(["x-terminal-emulator", "gnome-terminal", "konsole", "xfce4-terminal", "alacritty", "kitty", "xterm"].map(String::from));
    for term in candidates {
        let args: &[&str] = if term.contains("gnome-terminal") || term.contains("xfce4-terminal") { &["--"] } else { &["-e"] };
        let spawned = Command::new(&term)
            .args(args)
            .arg(exe)
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        if spawned.is_ok() {
            return Ok("new window");
        }
    }
    Err("no terminal emulator found (set $TERMINAL)".into())
}
