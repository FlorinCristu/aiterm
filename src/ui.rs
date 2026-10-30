//! Colours, glyphs and the chrome around the prompt: banner, prompt, spinner, status lines.
//!
//! One accent (the theme's green) marks everything the AI touches; the rest stays plain terminal ink.
//! Glyphs are limited to ones the stock Windows console fonts (Consolas, Lucida) can draw.

use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";
// Plain ANSI colours, so the loaded theme (see theme.rs) decides how they look.
pub const ACCENT: &str = "\x1b[32m";
pub const CWD: &str = "\x1b[34m";
pub const BRANCH: &str = "\x1b[35m";
pub const FAIL: &str = "\x1b[31m";
pub const MUTED: &str = "\x1b[90m";

pub const SPARK: &str = "♦";
pub const ARROW: &str = "›";

const WORDMARK: [&str; 2] = ["▄▀█ █ ▀█▀ █▀▀ █▀█ █▀▄▀█", "█▀█ █  █  ██▄ █▀▄ █ ▀ █"];

const EXAMPLES: &[&str] = &[
    "find files bigger than 100MB here",
    "what is listening on port 3000",
    "undo my last git commit but keep the changes",
    "count lines of rust code in this folder",
    "show the 10 newest files",
    "zip this folder without node_modules",
    "which branch did I work on yesterday",
    "replace tabs with spaces in all .py files",
];

pub fn banner(bash: &Path, model: Option<&str>) {
    let example = EXAMPLES[(seed() as usize) % EXAMPLES.len()];
    println!();
    for row in WORDMARK {
        println!("  {ACCENT}{BOLD}{row}{RESET}");
    }
    println!();
    println!("  Type a command, or just say what you want.");
    println!("  {MUTED}try{RESET}  {ACCENT}{example}{RESET}");
    match model {
        Some(m) => println!("  {MUTED}{SPARK} {m}  ·  bash {}  ·  ? for help{RESET}", bash.display()),
        None => println!(
            "  {FAIL}{SPARK} AI is off{RESET}{MUTED}  ·  type / to add your DeepSeek key  ·  ? for help{RESET}"
        ),
    }
    println!();
}

pub fn help() {
    let row = |left: &str, right: &str| println!("  {BOLD}{left:<16}{RESET}{MUTED}{right}{RESET}");
    println!();
    row("ls -la", "runs in bash, exactly as typed");
    println!("  {ACCENT}{:<16}{RESET}{MUTED}goes to the AI; you get a command to edit, Enter runs it{RESET}", "show big files");
    row("#anything", "always ask the AI");
    row("!anything", "always run it in bash");
    row("cd -", "back to the previous folder");
    row("/", "settings: add, replace or remove your DeepSeek key");
    println!();
    println!("  {MUTED}While you type, text turns {ACCENT}green{MUTED} when it will go to the AI.{RESET}");
    println!("  {MUTED}↑ ↓ history  ·  Tab completes paths  ·  Ctrl-C cancels  ·  exit quits{RESET}");
    println!();
}

/// The prompt as rustyline measures it (plain) and as it is drawn (coloured).
pub fn prompt(cwd: &str, branch: Option<&str>, last_failed: bool) -> (String, String) {
    let arrow_colour = if last_failed { FAIL } else { ACCENT };
    match branch {
        Some(b) => (
            format!("{cwd} {b} {ARROW} "),
            format!("{CWD}{BOLD}{cwd}{RESET} {BRANCH}{b}{RESET} {arrow_colour}{BOLD}{ARROW}{RESET} "),
        ),
        None => (format!("{cwd} {ARROW} "), format!("{CWD}{BOLD}{cwd}{RESET} {arrow_colour}{BOLD}{ARROW}{RESET} ")),
    }
}

pub fn set_title(cwd: &str) {
    print!("\x1b]0;aiterm · {cwd}\x07");
}

/// Current branch (or short commit when detached), read straight from `.git` so the prompt costs no process.
pub fn git_branch(cwd: &Path) -> Option<String> {
    let mut dir = Some(cwd);
    while let Some(d) = dir {
        let dot_git = d.join(".git");
        let git_dir = if dot_git.is_dir() {
            dot_git
        } else if dot_git.is_file() {
            let s = std::fs::read_to_string(&dot_git).ok()?;
            d.join(s.trim().strip_prefix("gitdir:")?.trim())
        } else {
            dir = d.parent();
            continue;
        };
        let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
        let head = head.trim();
        return Some(match head.strip_prefix("ref: refs/heads/") {
            Some(branch) => branch.to_string(),
            None => head.chars().take(7).collect(),
        });
    }
    None
}

/// How the last command ended: silent for quick successes, a short note otherwise.
pub fn report(code: i32, took: Duration) {
    let t = fmt_duration(took);
    match code {
        0 if took >= Duration::from_secs(2) => println!("{MUTED}done in {t}{RESET}"),
        0 => {}
        130 => println!("{MUTED}interrupted · {t}{RESET}"),
        c => println!("{FAIL}exit {c}{RESET}{MUTED} · {t}{RESET}"),
    }
}

pub fn fmt_duration(d: Duration) -> String {
    let s = d.as_secs_f32();
    if s < 60.0 {
        format!("{s:.1}s")
    } else {
        format!("{}m {:02}s", d.as_secs() / 60, d.as_secs() % 60)
    }
}

/// Bouncing dots on their own line until stopped; the line is cleared on stop.
pub struct Spinner {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Spinner {
    pub fn start(label: &'static str) -> Self {
        const FRAMES: [&str; 4] = ["●○○", "○●○", "○○●", "○●○"];
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let handle = std::thread::spawn(move || {
            let started = Instant::now();
            let mut out = std::io::stdout();
            let _ = write!(out, "\x1b[?25l");
            let mut i = 0;
            while !flag.load(Ordering::Relaxed) {
                let _ = write!(
                    out,
                    "\r\x1b[2K{ACCENT}{}{RESET} {MUTED}{label} {:.1}s{RESET}",
                    FRAMES[i % FRAMES.len()],
                    started.elapsed().as_secs_f32()
                );
                let _ = out.flush();
                i += 1;
                std::thread::sleep(Duration::from_millis(90));
            }
            let _ = write!(out, "\r\x1b[2K\x1b[?25h");
            let _ = out.flush();
        });
        Spinner { stop, handle: Some(handle) }
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.stop();
    }
}

fn seed() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
}
