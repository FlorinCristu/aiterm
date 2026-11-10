//! aiterm-setup.exe: the whole Windows install in one file. It unpacks the files embedded at build
//! time (see build.rs) to a fresh temp folder and runs their install.ps1, which stays the one
//! place that knows how to install.
//!   aiterm-setup.exe                  install, then offer to open aiterm
//!   aiterm-setup.exe /quiet           install with no message boxes; the exit code tells
//!   aiterm-setup.exe /extract <dir>   only unpack the files into <dir>
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod payload {
    include!(concat!(env!("OUT_DIR"), "/payload.rs"));
}

#[cfg(windows)]
fn main() {
    std::process::exit(win::run());
}

#[cfg(not(windows))]
fn main() {
    println!("aiterm-setup is the Windows installer; on Linux and macOS use the aiterm tarball.");
}

#[cfg(windows)]
mod win {
    use crate::payload::FILES;
    use std::os::windows::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::{env, fs, io, ptr};
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDYES, MB_ICONERROR, MB_ICONINFORMATION, MB_ICONQUESTION, MB_OK, MB_YESNO, MESSAGEBOX_STYLE,
    };

    const TITLE: &str = "aiterm setup";
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const USAGE: &str = "aiterm-setup.exe                   install aiterm\n\
                         aiterm-setup.exe /quiet            install without messages (exit code 0 = done)\n\
                         aiterm-setup.exe /extract <dir>    only unpack the files into <dir>";

    /// The process exit code: 0 done, 1 install failed, 2 usage or unpack error.
    pub fn run() -> i32 {
        let (mut quiet, mut extract) = (false, None);
        let mut args = env::args_os().skip(1);
        while let Some(a) = args.next() {
            match a.to_string_lossy().trim_start_matches(['/', '-']).to_ascii_lowercase().as_str() {
                "quiet" | "q" => quiet = true,
                "extract" => match args.next() {
                    Some(d) => extract = Some(PathBuf::from(d)),
                    None => return fail(quiet, 2, "/extract needs a folder.\n\n".to_string() + USAGE),
                },
                _ => return fail(quiet, 2, USAGE.into()),
            }
        }
        if FILES.is_empty() {
            return fail(quiet, 2, "This aiterm-setup.exe was built without aiterm's files; build it with packaging\\build.ps1.".into());
        }
        if let Some(dir) = extract {
            return match unpack(&dir) {
                Ok(()) => 0,
                Err(e) => fail(quiet, 2, format!("Couldn't unpack to {}: {e}", dir.display())),
            };
        }
        if let Err(e) = install() {
            return fail(quiet, 1, format!("aiterm could not be installed.\n\n{e}"));
        }
        if !quiet && message("aiterm is installed. Open it now?", MB_YESNO | MB_ICONQUESTION) == IDYES {
            if let Err(e) = open_app() {
                message(&format!("aiterm is installed, but couldn't be started: {e}"), MB_OK | MB_ICONERROR);
            }
        }
        0
    }

    fn fail(quiet: bool, code: i32, text: String) -> i32 {
        if !quiet {
            let icon = if text.ends_with(USAGE) { MB_ICONINFORMATION } else { MB_ICONERROR };
            message(&text, MB_OK | icon);
        }
        code
    }

    fn install() -> Result<(), String> {
        let dir = fresh_temp().map_err(|e| format!("Couldn't create a temporary folder: {e}"))?;
        let done = unpack(&dir).map_err(|e| format!("Couldn't unpack to {}: {e}", dir.display())).and_then(|()| run_script(&dir));
        let _ = fs::remove_dir_all(&dir);
        done
    }

    fn fresh_temp() -> io::Result<PathBuf> {
        let base = env::temp_dir();
        for i in 0.. {
            let dir = base.join(format!("aiterm-setup-{}-{i}", std::process::id()));
            match fs::create_dir(&dir) {
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                r => return r.map(|()| dir),
            }
        }
        unreachable!()
    }

    fn unpack(dir: &Path) -> io::Result<()> {
        fs::create_dir_all(dir)?;
        FILES.iter().try_for_each(|(name, data)| fs::write(dir.join(name), data))
    }

    /// install.ps1 -Quiet, hidden. On failure its one-line message (stderr) is the error.
    fn run_script(dir: &Path) -> Result<(), String> {
        // Full path: a powershell.exe next to this file (say in Downloads) must not be the one run.
        let windir = env::var_os("SystemRoot").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        let out = Command::new(windir.join(r"System32\WindowsPowerShell\v1.0\powershell.exe"))
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-File"])
            .arg(dir.join("install.ps1"))
            .arg("-Quiet")
            .current_dir(dir)
            .stdin(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("Couldn't start PowerShell: {e}"))?;
        if out.status.success() {
            return Ok(());
        }
        let err = String::from_utf8_lossy(&out.stderr);
        let text = if err.trim().is_empty() { String::from_utf8_lossy(&out.stdout) } else { err };
        match text.trim() {
            "" => Err(format!("install.ps1 failed with exit code {}.", out.status.code().unwrap_or(-1))),
            t => Err(t.to_string()),
        }
    }

    /// Start the installed window. install.ps1 records where it put aiterm in its uninstall entry.
    fn open_app() -> Result<(), String> {
        let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\aiterm");
        let value = wide("InstallLocation");
        let mut buf = [0u16; 1024];
        let mut size = std::mem::size_of_val(&buf) as u32;
        let rc = unsafe {
            RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr(), RRF_RT_REG_SZ, ptr::null_mut(), buf.as_mut_ptr().cast(), &mut size)
        };
        if rc != 0 {
            return Err("its install folder isn't recorded in the registry".into());
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let exe = PathBuf::from(String::from_utf16_lossy(&buf[..len])).join("aiterm-app.exe");
        let home = env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_else(env::temp_dir);
        Command::new(&exe).current_dir(home).spawn().map(drop).map_err(|e| format!("{}: {e}", exe.display()))
    }

    fn message(text: &str, style: MESSAGEBOX_STYLE) -> i32 {
        let (text, title) = (wide(text), wide(TITLE));
        unsafe { MessageBoxW(ptr::null_mut(), text.as_ptr(), title.as_ptr(), style) }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }
}
