//! The `/` settings menu: add, replace or remove the DeepSeek key without leaving aiterm.
//!
//! Key entry uses its own line editor with masked echo, so the key never reaches the
//! screen, the scrollback or ~/.aiterm_history.

use crate::ai;
use crate::ui::{AMBER, BOLD, CORAL, GRAY, RESET, SAGE, SPARK};
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
    println!("  {AMBER}{BOLD}{SPARK} settings{RESET}");
    let status = match client {
        Some(c) if ai::env_key().is_some() => format!("{SAGE}{}{RESET}{GRAY}  from DEEPSEEK_API_KEY{RESET}", c.masked_key()),
        Some(c) => format!("{SAGE}{}{RESET}{GRAY}  saved in ~/.aiterm_key{RESET}", c.masked_key()),
        None => format!("{CORAL}not set{RESET}"),
    };
    println!("  {BOLD}1{RESET}  DeepSeek key   {status}");
    if client.is_some() {
        println!("  {BOLD}2{RESET}  Remove key");
    }
    println!("  {GRAY}Enter closes{RESET}");
    println!();
    match read_plain("  choose › ").as_deref() {
        Some("1") => {
            set_key(client);
        }
        Some("2") if client.is_some() => remove_key(client),
        _ => {}
    }
}

/// Ask for a key, check it against DeepSeek, save it. Returns whether AI is now on.
pub fn set_key(client: &mut Option<ai::Client>) -> bool {
    println!("  {GRAY}Paste your key from platform.deepseek.com/api_keys (Enter to skip){RESET}");
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
            println!("  {CORAL}{SPARK} {e}; nothing saved.{RESET}");
            return false;
        }
        Err(e) => println!("  {GRAY}{SPARK} {e}; saving it anyway.{RESET}"),
        Ok(()) => {}
    }
    if let Err(e) = ai::save_key(&key) {
        println!("  {CORAL}{SPARK} could not save the key: {e}{RESET}");
        return false;
    }
    if ai::env_key().is_some() {
        println!("  {GRAY}Note: DEEPSEEK_API_KEY is set in your environment; it wins in new windows.{RESET}");
    }
    candidate.warm_up();
    println!("  {AMBER}{SPARK}{RESET} AI is on  {GRAY}·  {}  ·  saved in ~/.aiterm_key{RESET}", candidate.model());
    println!();
    *client = Some(candidate);
    true
}

fn remove_key(client: &mut Option<ai::Client>) {
    if let Err(e) = ai::remove_key() {
        println!("  {CORAL}{SPARK} could not remove ~/.aiterm_key: {e}{RESET}");
        return;
    }
    if ai::env_key().is_some() {
        println!("  {GRAY}Removed ~/.aiterm_key, but DEEPSEEK_API_KEY is still set in your environment.{RESET}");
        return;
    }
    *client = None;
    println!("  {GRAY}Key removed. AI is off.{RESET}");
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
