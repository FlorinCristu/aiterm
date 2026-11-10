mod ai;
mod cli;
mod danger;
mod detect;
mod font;
mod input;
mod install;
mod jump;
mod notify;
mod settings;
mod shell;
mod theme;
mod ui;
mod window;

use detect::{Detector, Kind};
use input::{Input, AI_PROMPT};
use rustyline::error::ReadlineError;
use rustyline::Editor;
use shell::{Outcome, Shell};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use ui::{ACCENT, BOLD, FAIL, MUTED, RESET, SPARK};

static CANCEL: AtomicBool = AtomicBool::new(false);

fn main() {
    enable_ansi();
    print!("\x1b]0;aiterm\x07"); // window/tab title
    let own_window = owns_console();
    theme::load().apply();
    font::watch_zoom_keys();
    // Ctrl-C belongs to the running child (or cancels an AI request); never kill the shell itself.
    let _ = ctrlc::set_handler(|| CANCEL.store(true, Ordering::Relaxed));

    let mut sh = Shell::new();
    let detector = Arc::new(Detector::new(&sh.tool_dirs(), &sh.applets()));
    let manager = install::Manager::detect(&detector);
    let mut client = ai::Backend::load();
    if let Some(c) = &client {
        c.warm_up();
    }

    let mut rl: Editor<Input, rustyline::history::DefaultHistory> = Editor::new().expect("terminal");
    rl.set_helper(Some(Input::new(detector.clone())));
    input::bind_keys(&mut rl);
    let history = ai::home_dir().map(|h| h.join(".aiterm_history"));
    if let Some(h) = &history {
        let _ = rl.load_history(h);
    }

    ui::banner(&sh, client.as_ref().map(|c| c.label()).as_deref());

    let mut last: Option<(String, i32)> = None;
    'repl: loop {
        CANCEL.store(false, Ordering::Relaxed);
        let cwd = shell::pretty_cwd(&sh.cwd);
        ui::set_title(&cwd);
        let _ = std::io::stdout().flush(); // the title would otherwise wait for the next newline
        window::set_cwd(&sh.cwd);
        let branch = ui::git_branch(&sh.cwd);
        let failed = matches!(&last, Some((_, c)) if *c != 0 && *c != 130);
        let (prompt, coloured) = ui::prompt(&cwd, branch.as_deref(), failed);
        if let Some(h) = rl.helper() {
            *h.prompt.borrow_mut() = (prompt.clone(), coloured);
        }
        let line = match rl.readline(&prompt) {
            Ok(l) => l,
            Err(ReadlineError::Interrupted) => continue,
            Err(_) => break,
        };
        let input = line.trim().trim_start_matches('\u{feff}');
        if input.is_empty() {
            continue;
        }
        if matches!(input, "/" | "/settings" | "/key") {
            settings::menu(&mut client);
            continue;
        }
        if let Some(kind) = match input {
            "/tab" | "/newtab" => Some(window::Kind::Tab),
            "/window" | "/new" | "/newwindow" => Some(window::Kind::Window),
            _ => None,
        } {
            match window::open(kind, &sh.cwd) {
                Ok(what) => println!("  {MUTED}{SPARK} opened a {what} in {cwd}{RESET}"),
                Err(e) => eprintln!("  {FAIL}{SPARK} couldn't open one: {e}{RESET}"),
            }
            continue;
        }
        if matches!(input, "/clear" | "clear" | "cls") {
            print!("\x1b[2J\x1b[3J\x1b[H");
            let _ = std::io::stdout().flush();
            continue;
        }
        if input == "?" {
            ui::help();
            continue;
        }
        let _ = rl.add_history_entry(input);

        // `?tar -xzf x.tgz` explains a command instead of running it; `??` explains the last one.
        if let Some(rest) = input.strip_prefix('?') {
            let rest = rest.trim_start_matches('?').trim();
            match (if rest.is_empty() { last.as_ref().map(|(c, _)| c.as_str()) } else { Some(rest) }, &client) {
                (None, _) => println!("  {MUTED}{SPARK} nothing to explain yet: run something first, or type ?command{RESET}"),
                (_, None) => println!("  {FAIL}{SPARK} AI is off.{RESET} {MUTED}Type / to pick one.{RESET}"),
                (Some(target), Some(client)) => explain(client, target, &sh, &last),
            }
            println!();
            continue;
        }

        let mut command = match detector.classify(input) {
            Kind::Shell => input.trim_start_matches('!').trim().to_string(),
            Kind::Natural => {
                let request = input.trim_start_matches('#').trim();
                if client.is_none() {
                    println!("  {FAIL}{SPARK} AI is off.{RESET} {MUTED}Pick an AI to ask this, or prefix with ! to run it as-is.{RESET}");
                    if !settings::choose_backend(&mut client) {
                        continue;
                    }
                }
                let Some(client) = &client else { continue };
                let mut suggestion = ask(&sh, &last, |ctx, tok| client.suggest(request, ctx, &CANCEL, tok));
                loop {
                    let Some(cmd) = suggestion else { continue 'repl };
                    if let Some(risk) = danger::warning(&cmd) {
                        println!("  {FAIL}{SPARK} careful: this {risk}{RESET}");
                    }
                    match rl.readline_with_initial(AI_PROMPT, (&cmd, "")) {
                        // `#only .js files` on the run line sends the command back to the AI to be changed.
                        Ok(edited) if edited.trim().starts_with('#') => {
                            let feedback = edited.trim().trim_start_matches('#').trim();
                            CANCEL.store(false, Ordering::Relaxed);
                            suggestion = ask(&sh, &last, |ctx, tok| client.revise(request, &cmd, feedback, ctx, &CANCEL, tok))
                                .or(Some(cmd));
                        }
                        Ok(edited) if !edited.trim().is_empty() => {
                            let _ = rl.add_history_entry(edited.trim());
                            break edited.trim().to_string();
                        }
                        _ => continue 'repl,
                    }
                }
            }
        };
        if command.is_empty() {
            continue;
        }

        loop {
            CANCEL.store(false, Ordering::Relaxed);
            let started = Instant::now();
            let code = match sh.run(&command) {
                Outcome::Exit => break 'repl,
                Outcome::Code(code) => code,
            };
            ui::report(code, started.elapsed());
            notify::done(started.elapsed());
            if code == 0 && install::is_install(&command) {
                install::refresh_path();
            }
            let fix = (code == 127).then(|| not_found(&command, &detector, manager)).flatten();
            last = Some((command, code));
            // Offer the install command on the editable line, like an AI suggestion.
            match fix.map(|f| rl.readline_with_initial(AI_PROMPT, (&f, ""))) {
                Some(Ok(edited)) if !edited.trim().is_empty() => {
                    let _ = rl.add_history_entry(edited.trim());
                    command = edited.trim().to_string();
                }
                _ => break,
            }
        }
        println!(); // breathing room between one command's output and the next prompt
    }
    if let Some(h) = &history {
        let _ = rl.save_history(h);
    }
    println!("  {MUTED}bye {ACCENT}{SPARK}{RESET}");
    if !own_window {
        theme::restore();
    }
}

/// After "command not found": say how to get the missing program and return that command.
fn not_found(command: &str, detector: &Detector, manager: install::Manager) -> Option<String> {
    let name = install::program(command)?;
    if detector.is_command(name) {
        return None; // it exists; the 127 came from something it ran
    }
    match install::hint(name, manager) {
        install::Hint::Install(cmd) => {
            println!("  {ACCENT}{SPARK}{RESET} {BOLD}{name}{RESET} {MUTED}isn't installed  ·  Enter installs it  ·  Ctrl-C skips{RESET}");
            Some(cmd)
        }
        install::Hint::Search(cmd) => {
            println!("  {ACCENT}{SPARK}{RESET} {MUTED}no install recipe for {BOLD}{name}{RESET}{MUTED}  ·  Enter searches for a package  ·  Ctrl-C skips{RESET}");
            Some(cmd)
        }
    }
}

fn context<R>(sh: &Shell, last: &Option<(String, i32)>, call: impl FnOnce(&ai::Context) -> R) -> R {
    let files = list_files(&sh.cwd);
    let cwd = sh.cwd.display().to_string();
    call(&ai::Context { cwd: &cwd, os: os_description(sh), files: &files, last: last.as_ref().map(|(c, code)| (c.as_str(), *code)) })
}

/// Stream the suggestion on a dim line, then erase it so the editable command replaces it.
fn ask(
    sh: &Shell,
    last: &Option<(String, i32)>,
    call: impl FnOnce(&ai::Context, &mut dyn FnMut(&str)) -> Result<String, String>,
) -> Option<String> {
    let started = Instant::now();
    let mut out = std::io::stdout();
    let mut spinner = Some(ui::Spinner::start("thinking"));
    let result = context(sh, last, |ctx| {
        call(ctx, &mut |tok| {
            if let Some(mut s) = spinner.take() {
                s.stop();
                let _ = write!(out, "  {ACCENT}{SPARK} {MUTED}");
            }
            let _ = write!(out, "{}", tok.replace('\n', " "));
            let _ = out.flush();
        })
    });
    drop(spinner);
    let _ = write!(out, "{RESET}\r\x1b[2K");
    match result {
        Ok(cmd) => {
            println!(
                "  {ACCENT}{SPARK}{RESET} {MUTED}{}  ·  Enter runs it  ·  edit it, or type #what to change  ·  Ctrl-C cancels{RESET}",
                ui::fmt_duration(started.elapsed())
            );
            Some(cmd)
        }
        Err(e) => {
            eprintln!("  {FAIL}{SPARK} {e}{RESET}");
            None
        }
    }
}

/// Say in plain words what `command` does. DeepSeek streams it; the CLIs deliver it in one piece.
fn explain(client: &ai::Backend, command: &str, sh: &Shell, last: &Option<(String, i32)>) {
    let mut out = std::io::stdout();
    let mut spinner = Some(ui::Spinner::start("reading"));
    let mut streamed = false;
    let result = context(sh, last, |ctx| {
        client.explain(command, ctx, &CANCEL, |tok| {
            if let Some(mut s) = spinner.take() {
                s.stop();
                let _ = write!(out, "  {ACCENT}{SPARK}{RESET} {MUTED}");
            }
            streamed = true;
            let _ = write!(out, "{}", tok.replace('\n', "\n    "));
            let _ = out.flush();
        })
    });
    drop(spinner);
    match result {
        Ok(_) if streamed => println!("{RESET}"),
        Ok(text) => println!("  {ACCENT}{SPARK}{RESET} {MUTED}{}{RESET}", text.replace('\n', "\n    ")),
        Err(e) => eprintln!("{RESET}\r\x1b[2K  {FAIL}{SPARK} {e}{RESET}"),
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

fn os_description(sh: &Shell) -> &'static str {
    if cfg!(windows) && sh.is_busybox() {
        "Windows, running BusyBox-w32's ash in bash mode (no arrays, no {a,b} brace expansion): \
         busybox applets with few GNU extensions (no grep -P, find -printf, date -d 'yesterday', \
         ls --time-style), paths look like C:/Users/... (never /c/...), \
         Windows .exe tools also on PATH (curl.exe, where.exe, powershell)"
    } else if cfg!(windows) {
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
        // Launched straight into conhost (the Start Menu shortcut), the window may start in a
        // font without › ♦ ▀ ●, which then draw as "?". Only touch a console nobody else shares.
        if owns_console() {
            let out = GetStdHandle(STD_OUTPUT_HANDLE);
            let mut font: CONSOLE_FONT_INFOEX = std::mem::zeroed();
            font.cbSize = std::mem::size_of::<CONSOLE_FONT_INFOEX>() as u32;
            font.dwFontSize = COORD { X: 0, Y: font::saved_size() };
            font.FontFamily = 54; // FF_MODERN | TMPF_TRUETYPE | TMPF_VECTOR, as conhost stores it
            font.FontWeight = 400;
            for (dst, src) in font.FaceName.iter_mut().zip("Consolas".encode_utf16()) {
                *dst = src;
            }
            SetCurrentConsoleFontEx(out, 0, &font);
        }
    }
}

#[cfg(not(windows))]
fn enable_ansi() {}

/// True when aiterm has the console window to itself (not started from cmd, PowerShell, …).
#[cfg(windows)]
fn owns_console() -> bool {
    let mut pids = [0u32; 2];
    unsafe { windows_sys::Win32::System::Console::GetConsoleProcessList(pids.as_mut_ptr(), 2) == 1 }
}

#[cfg(not(windows))]
fn owns_console() -> bool {
    false
}
