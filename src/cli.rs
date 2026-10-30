//! Claude Code and Codex as backends: run their CLIs headless, so requests go through the
//! subscription the user signed in to with the CLI's own browser login. aiterm never sees a token.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Claude,
    Codex,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Claude => "claude",
            Kind::Codex => "codex",
        }
    }

    /// What the user types to sign in with their browser.
    pub fn login_hint(self) -> &'static str {
        match self {
            Kind::Claude => "claude auth login",
            Kind::Codex => "codex login",
        }
    }

    pub fn install_hint(self) -> &'static str {
        match self {
            Kind::Claude => "npm install -g @anthropic-ai/claude-code",
            Kind::Codex => "npm install -g @openai/codex",
        }
    }

    fn login_args(self) -> &'static [&'static str] {
        match self {
            Kind::Claude => &["auth", "login"],
            Kind::Codex => &["login"],
        }
    }
}

pub struct Cli {
    pub kind: Kind,
    program: PathBuf,
    model: Option<String>,
}

impl Cli {
    /// Finds the CLI on PATH. Model from AITERM_CLAUDE_MODEL (default haiku) or AITERM_CODEX_MODEL.
    pub fn find(kind: Kind) -> Option<Self> {
        let model = match kind {
            Kind::Claude => Some(std::env::var("AITERM_CLAUDE_MODEL").unwrap_or_else(|_| "haiku".into())),
            Kind::Codex => std::env::var("AITERM_CODEX_MODEL").ok(),
        }
        .filter(|m| !m.trim().is_empty());
        Some(Cli { kind, program: which(kind.name())?, model })
    }

    /// `claude · haiku`, or just `codex` when it uses its own default model.
    pub fn label(&self) -> String {
        match &self.model {
            Some(m) => format!("{} · {m}", self.kind.name()),
            None => self.kind.name().to_string(),
        }
    }

    /// Runs the CLI's browser sign-in in this console. Returns whether it exited cleanly.
    pub fn login(&self) -> bool {
        Command::new(&self.program).args(self.kind.login_args()).status().map(|s| s.success()).unwrap_or(false)
    }

    /// Sends `prompt` on stdin and returns the reply. The CLI runs in the temp folder so it
    /// doesn't pick up the project's CLAUDE.md / AGENTS.md; the caller passes the context itself.
    pub fn ask(&self, prompt: &str, cancel: &AtomicBool) -> Result<String, String> {
        let name = self.kind.name();
        let reply_file = std::env::temp_dir().join(format!("aiterm-{name}-{}.txt", std::process::id()));
        let mut cmd = Command::new(&self.program);
        match self.kind {
            Kind::Claude => {
                // Everything that slows startup and isn't needed for a one-line answer is off.
                cmd.args(["-p", "--tools", "", "--no-session-persistence", "--strict-mcp-config"])
                    .args(["--setting-sources", "", "--disable-slash-commands"])
                    .args(["--system-prompt", "Follow the instructions in the user message exactly."]);
                if let Some(m) = &self.model {
                    cmd.args(["--model", m]);
                }
            }
            Kind::Codex => {
                cmd.args(["exec", "--skip-git-repo-check", "--sandbox", "read-only", "--color", "never"])
                    .args(["-c", "model_reasoning_effort=low", "--output-last-message"])
                    .arg(&reply_file);
                if let Some(m) = &self.model {
                    cmd.args(["-m", m]);
                }
                cmd.arg("-"); // prompt on stdin
            }
        }
        let mut child = cmd
            .current_dir(std::env::temp_dir())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("could not start {name} ({e})"))?;

        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(prompt.as_bytes());
        }
        let out = drain(child.stdout.take());
        let err = drain(child.stderr.take());

        let status = loop {
            if cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_file(&reply_file);
                return Err("cancelled".into());
            }
            match child.try_wait() {
                Ok(Some(s)) => break s,
                Ok(None) => std::thread::sleep(Duration::from_millis(30)),
                Err(e) => return Err(e.to_string()),
            }
        };
        let stdout = out.join().unwrap_or_default();
        let stderr = err.join().unwrap_or_default();
        let reply = std::fs::read_to_string(&reply_file).ok();
        let _ = std::fs::remove_file(&reply_file);

        if !status.success() {
            let last = stderr.lines().chain(stdout.lines()).map(str::trim).filter(|l| !l.is_empty()).last();
            let detail = last.unwrap_or("no output");
            let lower = detail.to_lowercase();
            return Err(if ["login", "log in", "auth", "401", "credential", "token"].iter().any(|w| lower.contains(w)) {
                format!("{name}: {detail}  ·  sign in with `{}` or from / settings", self.kind.login_hint())
            } else {
                format!("{name} failed: {detail}")
            });
        }
        Ok(reply.unwrap_or(stdout))
    }
}

fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(mut p) = pipe {
            let _ = p.read_to_string(&mut s);
        }
        s
    })
}

/// PATH lookup that also finds npm's `.cmd` shims on Windows, which `Command` won't find by bare name.
fn which(name: &str) -> Option<PathBuf> {
    let exts: &[&str] = if cfg!(windows) { &[".exe", ".cmd"] } else { &[""] };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .flat_map(|dir| exts.iter().map(move |ext| dir.join(format!("{name}{ext}"))))
        .find(|p| p.is_file())
}
