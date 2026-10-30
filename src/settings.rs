//! The `/` settings menu: the DeepSeek key and the colour theme, without leaving aiterm.
//!
//! Key entry uses its own line editor with masked echo, so the key never reaches the
//! screen, the scrollback or ~/.aiterm_history.

use crate::{ai, theme};
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

pub fn menu(client: &mut Option<ai::Client>) {
    println!();
    println!("  {ACCENT}{BOLD}{SPARK} settings{RESET}");
    let status = match client {
        Some(c) if ai::env_key().is_some() => format!("{BRANCH}{}{RESET}{MUTED}  from DEEPSEEK_API_KEY{RESET}", c.masked_key()),
        Some(c) => format!("{BRANCH}{}{RESET}{MUTED}  saved in ~/.aiterm_key{RESET}", c.masked_key()),
        None => format!("{FAIL}not set{RESET}"),
    };
    println!("  {BOLD}1{RESET}  DeepSeek key   {status}");
    if client.is_some() {
        println!("  {BOLD}2{RESET}  Remove key");
    }
    println!("  {BOLD}3{RESET}  Theme          {BRANCH}{}{RESET}{MUTED}  any theme from terminalcolors.com{RESET}", theme::load().name);
    println!("  {MUTED}Enter closes  ·  Ctrl+= / Ctrl+- / Ctrl+0 resize the text{RESET}");
    println!();
    match read_plain("  choose › ").as_deref() {
        Some("1") => {
            set_key(client);
        }
        Some("2") if client.is_some() => remove_key(client),
        Some("3") => set_theme(),
        _ => {}
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
pub fn set_key(client: &mut Option<ai::Client>) -> bool {
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
    candidate.warm_up();
    println!("  {ACCENT}{SPARK}{RESET} AI is on  {MUTED}·  {}  ·  saved in ~/.aiterm_key{RESET}", candidate.model());
    println!();
    *client = Some(candidate);
    true
}

fn remove_key(client: &mut Option<ai::Client>) {
    if let Err(e) = ai::remove_key() {
        println!("  {FAIL}{SPARK} could not remove ~/.aiterm_key: {e}{RESET}");
        return;
    }
    if ai::env_key().is_some() {
        println!("  {MUTED}Removed ~/.aiterm_key, but DEEPSEEK_API_KEY is still set in your environment.{RESET}");
        return;
    }
    *client = None;
    println!("  {MUTED}Key removed. AI is off.{RESET}");
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
