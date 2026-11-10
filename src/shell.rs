//! Runs commands through bash. `cd`, `export` and `exit` are handled here so their effect persists.
//!
//! On Windows that bash is Git Bash when installed, else busybox-w32 shipped next to aiterm.exe
//! (its `bash` applet is ash in bash-compatible mode), so aiterm works without Git for Windows.

use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Shell {
    program: PathBuf,
    kind: Kind,
    pub cwd: PathBuf,
    prev_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Bash,
    /// busybox-w32: run as `busybox.exe bash -c`; its applets stand in for coreutils.
    BusyBox,
    /// Nothing usable found: every command explains how to get a shell instead of failing oddly.
    Missing,
}

pub enum Outcome {
    Code(i32),
    Exit,
}

const NO_SHELL: &str = "no bash found. Install Git for Windows (winget install -e --id Git.Git), \
    or put busybox.exe next to aiterm.exe, or point AITERM_BASH at a bash.exe";

impl Shell {
    pub fn new() -> Self {
        let program = find_shell();
        let kind = match &program {
            None => Kind::Missing,
            Some(p) if is_busybox(p) => Kind::BusyBox,
            Some(_) => Kind::Bash,
        };
        Shell {
            program: program.unwrap_or_default(),
            kind,
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            prev_dir: None,
        }
    }

    pub fn is_busybox(&self) -> bool {
        self.kind == Kind::BusyBox
    }

    /// What the banner shows: which shell runs commands, or why none will.
    pub fn label(&self) -> String {
        match self.kind {
            Kind::Bash => format!("bash {}", self.program.display()),
            Kind::BusyBox => format!("busybox bash (built-in) {}", self.program.display()),
            Kind::Missing => NO_SHELL.to_string(),
        }
    }

    pub fn is_missing(&self) -> bool {
        self.kind == Kind::Missing
    }

    /// Extra directories whose tools bash can see but our own PATH may not (Git for Windows' usr/bin).
    pub fn tool_dirs(&self) -> Vec<PathBuf> {
        let Some(root) = self.program.parent().and_then(Path::parent) else { return vec![] };
        if cfg!(windows) && self.kind == Kind::Bash {
            vec![root.join("usr").join("bin"), root.join("mingw64").join("bin")]
        } else {
            vec![]
        }
    }

    /// Command names the shell provides without a file on PATH: busybox's applets (`ls`, `grep`, …),
    /// asked from the binary itself so the list always matches the shipped version.
    pub fn applets(&self) -> Vec<String> {
        if self.kind != Kind::BusyBox {
            return vec![];
        }
        match Command::new(&self.program).arg("--list").output() {
            Ok(o) if o.status.success() => parse_applets(&String::from_utf8_lossy(&o.stdout)),
            _ => vec![],
        }
    }

    /// `bash -c`, or `busybox bash -c`. busybox leaves HOME unset when Windows didn't set it, and
    /// then `~` and `cd` fail inside scripts, so it gets the profile folder (forward slashes, as
    /// busybox prints paths).
    fn command(&self) -> Command {
        let mut c = Command::new(&self.program);
        if self.kind == Kind::BusyBox {
            c.arg("bash");
            if std::env::var_os("HOME").is_none() {
                if let Some(h) = crate::ai::home_dir() {
                    c.env("HOME", h.to_string_lossy().replace('\\', "/"));
                }
            }
        }
        c
    }

    pub fn run(&mut self, line: &str) -> Outcome {
        let trimmed = line.trim();
        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let head = parts.next().unwrap_or("");
        let rest = parts.next().unwrap_or("").trim();
        match head {
            "exit" | "logout" => return Outcome::Exit,
            "cd" => return Outcome::Code(self.cd(rest)),
            "z" => return Outcome::Code(self.jump(rest)),
            "export" if !rest.is_empty() && !rest.contains(['$', '`', ';', '|', '&']) => {
                for assign in rest.split_whitespace() {
                    if let Some((k, v)) = assign.split_once('=') {
                        std::env::set_var(k, v.trim_matches(['"', '\'']));
                    }
                }
                return Outcome::Code(0);
            }
            _ => {}
        }
        if self.kind == Kind::Missing {
            eprintln!("aiterm: {NO_SHELL}");
            return Outcome::Code(126); // not 127: that would offer to install the command itself
        }
        let status = self.command().arg("-c").arg(trimmed).current_dir(&self.cwd).status();
        match status {
            Ok(s) => Outcome::Code(s.code().unwrap_or(130)),
            Err(e) => {
                eprintln!("aiterm: cannot start bash ({}): {e}", self.program.display());
                Outcome::Code(127)
            }
        }
    }

    /// `z proj`: cd to the best-remembered folder matching the words; bare `z` lists the favourites.
    fn jump(&mut self, query: &str) -> i32 {
        if query.is_empty() {
            for dir in crate::jump::top("", &self.cwd, 10) {
                println!("  {}", pretty_cwd(&dir));
            }
            return 0;
        }
        match crate::jump::find(query, &self.cwd) {
            Some(dir) => self.cd(&dir.to_string_lossy()),
            None => {
                eprintln!("z: no remembered folder matches '{query}' (folders are remembered as you cd into them)");
                1
            }
        }
    }

    fn cd(&mut self, arg: &str) -> i32 {
        let arg = arg.trim_matches(['"', '\'']);
        let target = match arg {
            "" | "~" => crate::ai::home_dir().unwrap_or_else(|| self.cwd.clone()),
            "-" => match &self.prev_dir {
                Some(p) => p.clone(),
                None => {
                    eprintln!("cd: OLDPWD not set");
                    return 1;
                }
            },
            a => {
                // busybox reads `/x` as `x` at the root of the current drive, which is what
                // joining onto cwd does; only Git Bash means `/c/x` as `C:/x`.
                let p = if let Some(rest) = a.strip_prefix("~/") {
                    crate::ai::home_dir().unwrap_or_default().join(rest)
                } else if self.kind == Kind::Bash {
                    msys_to_native(a)
                } else {
                    PathBuf::from(a)
                };
                self.cwd.join(p)
            }
        };
        match std::fs::canonicalize(&target) {
            Ok(p) if p.is_dir() => {
                let p = strip_verbatim(p);
                self.prev_dir = Some(std::mem::replace(&mut self.cwd, p));
                let _ = std::env::set_current_dir(&self.cwd);
                crate::jump::record(&self.cwd);
                0
            }
            _ => {
                eprintln!("cd: {arg}: No such directory");
                1
            }
        }
    }
}

/// `AITERM_BASH`, else (Windows) Git Bash, else the busybox.exe shipped beside aiterm.exe, else bash on PATH.
fn find_shell() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = vec![];
    if cfg!(windows) {
        candidates.push(r"C:\Program Files\Git\bin\bash.exe".into());
        candidates.push(r"C:\Program Files (x86)\Git\bin\bash.exe".into());
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            candidates.push(PathBuf::from(local).join(r"Programs\Git\bin\bash.exe"));
        }
        if let Some(dir) = std::env::current_exe().ok().as_deref().and_then(Path::parent) {
            candidates.push(dir.join("busybox.exe"));
        }
    }
    let exe = if cfg!(windows) { "bash.exe" } else { "bash" };
    let on_path: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).map(|d| d.join(exe)).filter(|p| p.is_file()).collect())
        .unwrap_or_default();
    let found = choose(std::env::var_os("AITERM_BASH").map(PathBuf::from), &candidates, |p| p.is_file(), &on_path);
    // Elsewhere keep the old behaviour: let the OS look `bash` up and complain if it must.
    found.or_else(|| (!cfg!(windows)).then(|| "bash".into()))
}

/// The shell selection rule, kept free of the file system for tests: an explicit `AITERM_BASH` wins
/// as given, then the first existing candidate, then bash on PATH that isn't the WSL launcher.
fn choose(env: Option<PathBuf>, candidates: &[PathBuf], exists: impl Fn(&Path) -> bool, on_path: &[PathBuf]) -> Option<PathBuf> {
    env.filter(|p| !p.as_os_str().is_empty())
        .or_else(|| candidates.iter().find(|p| exists(p)).cloned())
        .or_else(|| on_path.iter().find(|p| !is_wsl_launcher(p)).cloned())
}

/// `C:\Windows\System32\bash.exe` (and the WindowsApps alias) start WSL, or an install prompt
/// when no distro is set up: never a usable Windows bash.
fn is_wsl_launcher(p: &Path) -> bool {
    let s = p.to_string_lossy().to_ascii_lowercase().replace('/', "\\");
    ["\\windows\\system32\\", "\\windows\\sysnative\\", "\\windows\\syswow64\\", "\\microsoft\\windowsapps\\"]
        .iter()
        .any(|d| s.contains(d))
}

/// busybox is recognised by file name (`busybox.exe`, `busybox64u.exe`, …), so `AITERM_BASH`
/// pointing at one gets the busybox treatment too.
fn is_busybox(p: &Path) -> bool {
    p.file_name().map_or(false, |n| n.to_string_lossy().to_ascii_lowercase().starts_with("busybox"))
}

/// `busybox --list` prints one applet per line.
fn parse_applets(list: &str) -> Vec<String> {
    list.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect()
}

/// `/c/Users/x` -> `C:/Users/x` on Windows so Git-bash style paths work with `cd`.
fn msys_to_native(p: &str) -> PathBuf {
    if cfg!(windows) {
        let b = p.as_bytes();
        if b.len() >= 2 && b[0] == b'/' && b[1].is_ascii_alphabetic() && (b.len() == 2 || b[2] == b'/') {
            return PathBuf::from(format!("{}:/{}", (b[1] as char).to_ascii_uppercase(), p.get(3..).unwrap_or("")));
        }
    }
    PathBuf::from(p)
}

/// canonicalize() on Windows yields `\\?\C:\...`; bash and humans prefer `C:\...`.
fn strip_verbatim(p: PathBuf) -> PathBuf {
    match p.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        Some(s) if !s.starts_with("UNC") => PathBuf::from(s),
        _ => p,
    }
}

/// Short, readable cwd for the prompt: `~/code/x`.
pub fn pretty_cwd(cwd: &Path) -> String {
    let s = cwd.display().to_string().replace('\\', "/");
    if let Some(home) = crate::ai::home_dir() {
        let h = home.display().to_string().replace('\\', "/");
        if let Some(rest) = s.strip_prefix(&h) {
            return format!("~{rest}");
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pb(v: &[&str]) -> Vec<PathBuf> {
        v.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn selection() {
        let cands = pb(&[r"C:\Program Files\Git\bin\bash.exe", r"C:\aiterm\busybox.exe"]);
        let path = pb(&[r"C:\Windows\System32\bash.exe", r"D:\msys\usr\bin\bash.exe"]);
        let has = |set: &'static [&'static str]| move |p: &Path| set.iter().any(|s| Path::new(s) == p);
        // AITERM_BASH wins even when it doesn't exist (it may be a bare name).
        assert_eq!(choose(Some("x/busybox.exe".into()), &cands, has(&[]), &path), Some("x/busybox.exe".into()));
        assert_eq!(choose(Some("".into()), &cands, has(&[r"C:\aiterm\busybox.exe"]), &path), Some(cands[1].clone()));
        // Git Bash beats the bundled busybox; busybox beats PATH.
        let both = has(&[r"C:\Program Files\Git\bin\bash.exe", r"C:\aiterm\busybox.exe"]);
        assert_eq!(choose(None, &cands, both, &path), Some(cands[0].clone()));
        assert_eq!(choose(None, &cands, has(&[r"C:\aiterm\busybox.exe"]), &path), Some(cands[1].clone()));
        // Nothing installed: PATH bash, skipping the WSL launcher; only the launcher: nothing.
        assert_eq!(choose(None, &cands, has(&[]), &path), Some(path[1].clone()));
        assert_eq!(choose(None, &cands, has(&[]), &path[..1]), None);
    }

    #[test]
    fn wsl_launcher() {
        assert!(is_wsl_launcher(Path::new(r"C:\Windows\System32\bash.exe")));
        assert!(is_wsl_launcher(Path::new("c:/windows/system32/BASH.EXE")));
        assert!(is_wsl_launcher(Path::new(r"C:\Users\x\AppData\Local\Microsoft\WindowsApps\bash.exe")));
        assert!(!is_wsl_launcher(Path::new(r"C:\Program Files\Git\usr\bin\bash.exe")));
        assert!(!is_wsl_launcher(Path::new("/usr/bin/bash")));
    }

    #[test]
    fn busybox_name() {
        assert!(is_busybox(Path::new(r"C:\tools\busybox.exe")));
        assert!(is_busybox(Path::new(r"C:\tools\BusyBox64u.exe")));
        assert!(!is_busybox(Path::new(r"C:\Program Files\Git\bin\bash.exe")));
        assert!(!is_busybox(Path::new(r"C:\busybox\bash.exe")));
    }

    #[test]
    fn applet_list() {
        assert_eq!(parse_applets("[\r\n[[\nls\n\n  grep \n"), ["[", "[[", "ls", "grep"]);
        assert!(parse_applets("").is_empty());
    }
}
