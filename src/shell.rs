//! Runs commands through bash. `cd`, `export` and `exit` are handled here so their effect persists.

use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Shell {
    pub bash: PathBuf,
    pub cwd: PathBuf,
    prev_dir: Option<PathBuf>,
}

pub enum Outcome {
    Code(i32),
    Exit,
}

impl Shell {
    pub fn new() -> Self {
        Shell {
            bash: find_bash(),
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            prev_dir: None,
        }
    }

    /// Extra directories whose tools bash can see but our own PATH may not (Git for Windows' usr/bin).
    pub fn tool_dirs(&self) -> Vec<PathBuf> {
        let Some(root) = self.bash.parent().and_then(Path::parent) else { return vec![] };
        if cfg!(windows) {
            vec![root.join("usr").join("bin"), root.join("mingw64").join("bin")]
        } else {
            vec![]
        }
    }

    pub fn run(&mut self, line: &str) -> Outcome {
        let trimmed = line.trim();
        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let head = parts.next().unwrap_or("");
        let rest = parts.next().unwrap_or("").trim();
        match head {
            "exit" | "logout" => return Outcome::Exit,
            "cd" => return Outcome::Code(self.cd(rest)),
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
        let status = Command::new(&self.bash).arg("-c").arg(trimmed).current_dir(&self.cwd).status();
        match status {
            Ok(s) => Outcome::Code(s.code().unwrap_or(130)),
            Err(e) => {
                eprintln!("aiterm: cannot start bash ({}): {e}", self.bash.display());
                Outcome::Code(127)
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
                let p = if let Some(rest) = a.strip_prefix("~/") {
                    crate::ai::home_dir().unwrap_or_default().join(rest)
                } else {
                    msys_to_native(a)
                };
                self.cwd.join(p)
            }
        };
        match std::fs::canonicalize(&target) {
            Ok(p) if p.is_dir() => {
                let p = strip_verbatim(p);
                self.prev_dir = Some(std::mem::replace(&mut self.cwd, p));
                let _ = std::env::set_current_dir(&self.cwd);
                0
            }
            _ => {
                eprintln!("cd: {arg}: No such directory");
                1
            }
        }
    }
}

fn find_bash() -> PathBuf {
    if let Some(p) = std::env::var_os("AITERM_BASH") {
        return p.into();
    }
    if cfg!(windows) {
        for c in [r"C:\Program Files\Git\bin\bash.exe", r"C:\Program Files (x86)\Git\bin\bash.exe"] {
            if Path::new(c).exists() {
                return c.into();
            }
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            let p = PathBuf::from(local).join(r"Programs\Git\bin\bash.exe");
            if p.exists() {
                return p;
            }
        }
    }
    "bash".into()
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
