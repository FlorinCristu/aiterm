//! The `/` settings menu: which AI answers, the DeepSeek key and the colour theme, without leaving aiterm.
//!
//! Key entry uses its own line editor with masked echo, so the key never reaches the
//! screen, the scrollback or ~/.aiterm_history.

use crate::{ai, cli, theme};
use crate::ui::{ACCENT, BOLD, FAIL, MUTED, RESET, BRANCH, SPARK};
use rustyline::completion::Completer;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{Editor, Helper};
use std::borrow::Cow;

/// Draws every typed character as a bullet.
struct Masked;

impl Helper for Masked {}
impl Validator for Masked {}
impl Completer for Masked {
    type Candidate = String;
}
impl Hinter for Masked {
    type Hint = String;
}
impl Highlighter for Masked {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        Cow::Owned("•".repeat(line.chars().count()))
    }
    fn highlight_char(&self, _line: &str, _pos: usize, _forced: bool) -> bool {
        true
    }
}

pub fn menu(backend: &mut Option<ai::Backend>) {
    println!();
    println!("  {ACCENT}{BOLD}{SPARK} settings{RESET}");
    let using = match backend {
        Some(b) => format!("{BRANCH}{}{RESET}", b.label()),
        None => format!("{FAIL}off{RESET}"),
    };
    println!("  {BOLD}1{RESET}  AI             {using}{MUTED}  DeepSeek, or your Claude / ChatGPT subscription{RESET}");
    let key = ai::Client::from_env().map(|c| c.masked_key());
    let key_status = match &key {
        Some(k) if ai::env_key().is_some() => format!("{BRANCH}{k}{RESET}{MUTED}  from DEEPSEEK_API_KEY{RESET}"),
        Some(k) => format!("{BRANCH}{k}{RESET}{MUTED}  saved in ~/.aiterm_key{RESET}"),
        None => format!("{MUTED}not set{RESET}"),
    };
    println!("  {BOLD}2{RESET}  DeepSeek key   {key_status}");
    if key.is_some() {
        println!("  {BOLD}3{RESET}  Remove key");
    }
    println!("  {BOLD}4{RESET}  Theme          {BRANCH}{}{RESET}{MUTED}  any theme from terminalcolors.com{RESET}", theme::load().name);
    if let Some(ai::Backend::Cli(c)) = backend {
        println!("  {BOLD}5{RESET}  Sign in        {MUTED}opens your browser via `{}`{RESET}", c.kind.login_hint());
    }
    println!("  {MUTED}Enter closes  ·  Ctrl+= / Ctrl+- / Ctrl+0 resize the text{RESET}");
    println!();
    match read_plain("  choose › ").as_deref() {
        Some("1") => {
            choose_backend(backend);
        }
        Some("2") => {
            set_key(backend);
        }
        Some("3") if key.is_some() => remove_key(backend),
        Some("4") => set_theme(),
        Some("5") => {
            if let Some(ai::Backend::Cli(c)) = backend {
                sign_in(c);
            }
        }
        _ => {}
    }
}

/// Pick DeepSeek or a subscription CLI and save the choice. Returns whether AI is now on.
pub fn choose_backend(backend: &mut Option<ai::Backend>) -> bool {
    println!("  {BOLD}1{RESET}  DeepSeek   {MUTED}API key  ·  fastest, about half a second{RESET}");
    println!("  {BOLD}2{RESET}  Claude     {MUTED}your Claude Pro/Max plan, through the claude CLI  ·  a few seconds{RESET}");
    println!("  {BOLD}3{RESET}  Codex      {MUTED}your ChatGPT plan, through the codex CLI  ·  a few seconds{RESET}");
    let kind = match read_plain(&format!("  {SPARK} AI › ")).as_deref() {
        Some("1") => None,
        Some("2") => Some(cli::Kind::Claude),
        Some("3") => Some(cli::Kind::Codex),
        _ => return backend.is_some(),
    };
    let Some(kind) = kind else {
        save_choice(None);
        *backend = ai::Client::from_env().map(ai::Backend::DeepSeek);
        if backend.is_none() {
            return set_key(backend);
        }
        on(backend);
        return true;
    };
    let Some(c) = cli::Cli::find(kind) else {
        println!("  {FAIL}{SPARK} the {} CLI isn't on PATH.{RESET} {MUTED}Install it with: {}{RESET}", kind.name(), kind.install_hint());
        println!();
        return backend.is_some();
    };
    save_choice(Some(kind));
    println!("  {MUTED}aiterm uses the {} CLI's own sign-in; it never sees your password or token.{RESET}", kind.name());
    if read_plain(&format!("  {MUTED}Sign in with your browser now? y signs in, Enter skips if you already are ›{RESET} "))
        .is_some_and(|a| a.eq_ignore_ascii_case("y"))
    {
        sign_in(&c);
    }
    *backend = Some(ai::Backend::Cli(c));
    on(backend);
    true
}

fn sign_in(c: &cli::Cli) {
    if c.login() {
        println!("  {ACCENT}{SPARK}{RESET} signed in");
    } else {
        println!("  {FAIL}{SPARK} sign-in didn't finish.{RESET} {MUTED}You can also run `{}` yourself.{RESET}", c.kind.login_hint());
    }
    println!();
}

fn save_choice(kind: Option<cli::Kind>) {
    if let Err(e) = ai::save_choice(kind) {
        println!("  {FAIL}{SPARK} could not save the choice in ~/.aiterm_backend: {e}{RESET}");
    }
    if std::env::var("AITERM_BACKEND").is_ok() {
        println!("  {MUTED}Note: AITERM_BACKEND is set in your environment; it wins in new windows.{RESET}");
    }
}

fn on(backend: &Option<ai::Backend>) {
    if let Some(b) = backend {
        b.warm_up();
        println!("  {ACCENT}{SPARK}{RESET} AI is on  {MUTED}·  {}{RESET}", b.label());
        println!();
    }
}

fn set_theme() {
    println!("  {MUTED}Paste a terminalcolors.com theme link, or the path of a downloaded theme file{RESET}");
    println!("  {MUTED}(Ghostty, Alacritty, Warp or iTerm2). Type deus to go back to the default.{RESET}");
    let Some(source) = read_plain(&format!("  {SPARK} theme › ")) else { return };
    if source.is_empty() {
        return;
    }
    let loaded = if source.eq_ignore_ascii_case("deus") {
        Ok(theme::deus())
    } else {
        let mut spinner = crate::ui::Spinner::start("fetching theme");
        let t = theme::fetch(&source);
        spinner.stop();
        t
    };
    match loaded {
        Ok(t) => {
            t.apply();
            match theme::save(&t) {
                Ok(()) => println!("  {ACCENT}{SPARK}{RESET} {} is on  {MUTED}·  saved in ~/.aiterm_theme{RESET}", t.name),
                Err(e) => println!("  {FAIL}{SPARK} {} is on for now, but could not be saved: {e}{RESET}", t.name),
            }
            println!();
        }
        Err(e) => println!("  {FAIL}{SPARK} {e}{RESET}"),
    }
}

/// Ask for a key, check it against DeepSeek, save it. Returns whether AI is now on.
/// The key only takes over when DeepSeek is the chosen backend.
pub fn set_key(backend: &mut Option<ai::Backend>) -> bool {
    println!("  {MUTED}Paste your key from platform.deepseek.com/api_keys (Enter to skip){RESET}");
    let Some(key) = read_masked(&format!("  {SPARK} key › ")) else { return false };
    let key = key.trim().to_string();
    if key.is_empty() {
        return false;
    }
    let candidate = ai::Client::with_key(key.clone());
    let mut spinner = crate::ui::Spinner::start("checking key");
    let verdict = candidate.check();
    spinner.stop();
    match verdict {
        Err(e) if e.contains("rejected") => {
            println!("  {FAIL}{SPARK} {e}; nothing saved.{RESET}");
            return false;
        }
        Err(e) => println!("  {MUTED}{SPARK} {e}; saving it anyway.{RESET}"),
        Ok(()) => {}
    }
    if let Err(e) = ai::save_key(&key) {
        println!("  {FAIL}{SPARK} could not save the key: {e}{RESET}");
        return false;
    }
    if ai::env_key().is_some() {
        println!("  {MUTED}Note: DEEPSEEK_API_KEY is set in your environment; it wins in new windows.{RESET}");
    }
    if ai::chosen().is_some() {
        println!("  {ACCENT}{SPARK}{RESET} key saved in ~/.aiterm_key  {MUTED}·  pick DeepSeek under 1 to use it{RESET}");
        println!();
        return backend.is_some();
    }
    candidate.warm_up();
    println!("  {ACCENT}{SPARK}{RESET} AI is on  {MUTED}·  {}  ·  saved in ~/.aiterm_key{RESET}", candidate.model());
    println!();
    *backend = Some(ai::Backend::DeepSeek(candidate));
    true
}

fn remove_key(backend: &mut Option<ai::Backend>) {
    if let Err(e) = ai::remove_key() {
        println!("  {FAIL}{SPARK} could not remove ~/.aiterm_key: {e}{RESET}");
        return;
    }
    if ai::env_key().is_some() {
        println!("  {MUTED}Removed ~/.aiterm_key, but DEEPSEEK_API_KEY is still set in your environment.{RESET}");
        return;
    }
    if matches!(backend, Some(ai::Backend::DeepSeek(_))) {
        *backend = None;
        println!("  {MUTED}Key removed. AI is off.{RESET}");
    } else {
        println!("  {MUTED}Key removed.{RESET}");
    }
    println!();
}

fn read_plain(prompt: &str) -> Option<String> {
    let mut rl: Editor<(), rustyline::history::DefaultHistory> = Editor::new().ok()?;
    rl.readline(prompt).ok().map(|s| s.trim().to_string())
}

fn read_masked(prompt: &str) -> Option<String> {
    let mut rl: Editor<Masked, rustyline::history::DefaultHistory> = Editor::new().ok()?;
    rl.set_helper(Some(Masked));
    rl.readline(prompt).ok()
}
