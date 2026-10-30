mod ai;
mod detect;
mod shell;

use detect::{Detector, Kind};
use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::{Hint, Hinter};
use rustyline::validate::Validator;
use rustyline::{Editor, Helper};
use shell::{Outcome, Shell};
use std::borrow::Cow;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

const DIM: &str = "\x1b[2m";
const CYAN: &str = "\x1b[36m";
const MAGENTA: &str = "\x1b[35m";
const RED: &str = "\x1b[31m";
const RESET: &str = "\x1b[0m";
const AI_PROMPT: &str = "✦ run ❯ ";

static CANCEL: AtomicBool = AtomicBool::new(false);

struct AiHint(&'static str);
impl Hint for AiHint {
    fn display(&self) -> &str {
        self.0
    }
    fn completion(&self) -> Option<&str> {
        None
    }
}

struct Input {
    detector: Arc<Detector>,
    files: FilenameCompleter,
}

impl Helper for Input {}
impl Validator for Input {}

impl Completer for Input {
    type Candidate = Pair;
    fn complete(&self, line: &str, pos: usize, ctx: &rustyline::Context<'_>) -> rustyline::Result<(usize, Vec<Pair>)> {
        self.files.complete(line, pos, ctx)
    }
}

/// Live indicator while typing: shows when Enter will go to the AI instead of bash.
impl Hinter for Input {
    type Hint = AiHint;
    fn hint(&self, line: &str, pos: usize, _: &rustyline::Context<'_>) -> Option<AiHint> {
        if pos < line.len() || line.trim().is_empty() {
            return None;
        }
        (self.detector.classify(line) == Kind::Natural).then_some(AiHint("   ✦ AI"))
    }
}

impl Highlighter for Input {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(&'s self, prompt: &'p str, _default: bool) -> Cow<'b, str> {
        if prompt.starts_with('✦') {
            return Cow::Owned(format!("{MAGENTA}{prompt}{RESET}"));
        }
        match prompt.rsplit_once(' ').and_then(|(p, _)| p.rsplit_once(' ')) {
            Some((dir, arrow)) => Cow::Owned(format!("{CYAN}{dir}{RESET} {MAGENTA}{arrow}{RESET} ")),
            None => Cow::Borrowed(prompt),
        }
    }
    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Cow::Owned(format!("{MAGENTA}{DIM}{hint}{RESET}"))
    }
}

fn main() {
    enable_ansi();
    print!("\x1b]0;aiterm\x07"); // window/tab title
    // Ctrl-C belongs to the running child (or cancels an AI request); never kill the shell itself.
    let _ = ctrlc::set_handler(|| CANCEL.store(true, Ordering::Relaxed));

    let mut sh = Shell::new();
    let detector = Arc::new(Detector::new(&sh.tool_dirs()));
    let client = ai::Client::from_env();
    if let Some(c) = &client {
        c.warm_up();
    }

    let mut rl: Editor<Input, rustyline::history::DefaultHistory> = Editor::new().expect("terminal");
    rl.set_helper(Some(Input { detector: detector.clone(), files: FilenameCompleter::new() }));
    let history = ai::home_dir().map(|h| h.join(".aiterm_history"));
    if let Some(h) = &history {
        let _ = rl.load_history(h);
    }

    println!(
        "{DIM}aiterm · bash: {} · ai: {} · type plain English to get a command (#force AI, !force shell){RESET}",
        sh.bash.display(),
        client.as_ref().map(|c| c.model().to_string()).unwrap_or_else(|| "off (set DEEPSEEK_API_KEY)".into())
    );

    let mut last: Option<(String, i32)> = None;
    loop {
        CANCEL.store(false, Ordering::Relaxed);
        let prompt = format!("{} ❯ ", shell::pretty_cwd(&sh.cwd));
        let line = match rl.readline(&prompt) {
            Ok(l) => l,
            Err(ReadlineError::Interrupted) => continue,
            Err(_) => break,
        };
        let input = line.trim().trim_start_matches('\u{feff}');
        if input.is_empty() {
            continue;
        }
        let _ = rl.add_history_entry(input);

        let command = match detector.classify(input) {
            Kind::Shell => input.trim_start_matches('!').trim().to_string(),
            Kind::Natural => {
                let request = input.trim_start_matches('#').trim();
                let Some(client) = &client else {
                    eprintln!("{RED}✦ AI is off.{RESET} Set DEEPSEEK_API_KEY (or put the key in ~/.aiterm_key), or prefix with ! to run as-is.");
                    continue;
                };
                match ask(client, request, &sh, &last) {
                    Some(cmd) => match rl.readline_with_initial(AI_PROMPT, (&cmd, "")) {
                        Ok(edited) if !edited.trim().is_empty() => {
                            let _ = rl.add_history_entry(edited.trim());
                            edited.trim().to_string()
                        }
                        _ => continue,
                    },
                    None => continue,
                }
            }
        };
        if command.is_empty() {
            continue;
        }

        CANCEL.store(false, Ordering::Relaxed);
        match sh.run(&command) {
            Outcome::Exit => break,
            Outcome::Code(code) => {
                if code != 0 {
                    println!("{DIM}[exit {code}]{RESET}");
                }
                last = Some((command, code));
            }
        }
    }
    if let Some(h) = &history {
        let _ = rl.save_history(h);
    }
}

/// Stream the suggestion on a dim line, then erase it so the editable command replaces it.
fn ask(client: &ai::Client, request: &str, sh: &Shell, last: &Option<(String, i32)>) -> Option<String> {
    let files = list_files(&sh.cwd);
    let cwd = sh.cwd.display().to_string();
    let ctx = ai::Context {
        cwd: &cwd,
        os: os_description(),
        files: &files,
        last: last.as_ref().map(|(c, code)| (c.as_str(), *code)),
    };
    let started = Instant::now();
    let mut out = std::io::stdout();
    let _ = write!(out, "{MAGENTA}{DIM}✦ thinking…{RESET}");
    let _ = out.flush();
    let mut first = true;
    let result = client.suggest(request, &ctx, &CANCEL, |tok| {
        if first {
            let _ = write!(out, "\r\x1b[2K{MAGENTA}{DIM}✦ ");
            first = false;
        }
        let _ = write!(out, "{}", tok.replace('\n', " "));
        let _ = out.flush();
    });
    let _ = write!(out, "{RESET}\r\x1b[2K");
    match result {
        Ok(cmd) => {
            println!("{DIM}✦ {:.2}s · Enter to run, edit first if you like, Ctrl-C to cancel{RESET}", started.elapsed().as_secs_f32());
            Some(cmd)
        }
        Err(e) => {
            eprintln!("{RED}✦ {e}{RESET}");
            None
        }
    }
}

fn list_files(dir: &std::path::Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    let mut v: Vec<String> = rd
        .flatten()
        .take(60)
        .map(|e| {
            let mut n = e.file_name().to_string_lossy().into_owned();
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                n.push('/');
            }
            n
        })
        .collect();
    v.sort();
    v
}

fn os_description() -> &'static str {
    if cfg!(windows) {
        "Windows, running Git Bash (MSYS2): GNU coreutils available, paths look like /c/Users/..., Windows .exe tools also on PATH"
    } else if cfg!(target_os = "macos") {
        "macOS (BSD userland)"
    } else {
        "Linux (GNU userland)"
    }
}

#[cfg(windows)]
fn enable_ansi() {
    use windows_sys::Win32::System::Console::*;
    unsafe {
        for h in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            let handle = GetStdHandle(h);
            let mut mode = 0;
            if GetConsoleMode(handle, &mut mode) != 0 {
                SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
            }
        }
    }
}

#[cfg(not(windows))]
fn enable_ansi() {}
