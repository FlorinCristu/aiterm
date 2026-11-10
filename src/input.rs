//! The line editor's helper: Tab completion, the grey history suggestion, the AI hint and colouring.
//!
//! Everything here runs on a keystroke, so it stays in memory: no processes, no directory walks,
//! except reading `.git/refs` when Tab is pressed after `git checkout` and friends.

use crate::detect::{Detector, Kind};
use crate::ui::{self, ACCENT, ARROW, BOLD, MUTED, RESET, SPARK};
use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::{Hint, Hinter};
use rustyline::history::{DefaultHistory, SearchDirection};
use rustyline::validate::Validator;
use rustyline::{Cmd, ConditionalEventHandler, Context, Editor, Event, EventContext, EventHandler, Helper};
use rustyline::{KeyCode, KeyEvent, Modifiers, RepeatCount};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

pub const AI_PROMPT: &str = "  ♦ run › ";
const AI_HINT: &str = "   ♦ ask AI";

/// Subcommands whose next words are usually a branch or tag.
const GIT_REF_CMDS: &[&str] =
    &["checkout", "switch", "merge", "rebase", "branch", "diff", "log", "cherry-pick", "reset"];

/// What shows after the cursor: the AI marker (nothing to accept) or the rest of a past command.
pub enum Suggestion {
    Ai,
    Ghost(String),
}

impl Hint for Suggestion {
    fn display(&self) -> &str {
        match self {
            Suggestion::Ai => AI_HINT,
            Suggestion::Ghost(s) => s,
        }
    }
    fn completion(&self) -> Option<&str> {
        match self {
            Suggestion::Ai => None,
            Suggestion::Ghost(s) => Some(s),
        }
    }
}

pub struct Input {
    detector: Arc<Detector>,
    files: FilenameCompleter,
    /// (plain, coloured) form of the current prompt.
    pub prompt: RefCell<(String, String)>,
}

impl Input {
    pub fn new(detector: Arc<Detector>) -> Self {
        Input { detector, files: FilenameCompleter::new(), prompt: RefCell::new((String::new(), String::new())) }
    }
}

impl Helper for Input {}
impl Validator for Input {}

/// Command names in the first word, branches after `git checkout` and co., file names everywhere.
impl Completer for Input {
    type Candidate = Pair;
    fn complete(&self, line: &str, pos: usize, ctx: &Context<'_>) -> rustyline::Result<(usize, Vec<Pair>)> {
        let before = &line[..pos];
        if let Some(start) = command_word(before) {
            let names = self.detector.commands_with_prefix(&before[start..]);
            if !names.is_empty() {
                let pairs = names.into_iter().map(|n| Pair { display: n.to_string(), replacement: format!("{n} ") });
                return Ok((start, pairs.collect()));
            }
        }
        let (start, files) = self.files.complete(line, pos, ctx)?;
        let Some(word) = ref_word(before) else { return Ok((start, files)) };
        let prefix = &before[word..];
        let refs = std::env::current_dir().ok().and_then(|d| ui::git_dir(&d)).map(|g| git_refs(&g)).unwrap_or_default();
        let mut out: Vec<Pair> = refs
            .into_iter()
            .filter(|r| r.starts_with(prefix))
            .map(|r| Pair { display: r.clone(), replacement: r })
            .collect();
        if out.is_empty() {
            return Ok((start, files));
        }
        if start == word {
            out.extend(files);
        }
        let mut seen = HashSet::new();
        out.retain(|p| seen.insert(p.replacement.clone()));
        Ok((word, out))
    }
}

/// English gets the AI marker; a command gets the rest of the newest history entry it starts.
impl Hinter for Input {
    type Hint = Suggestion;
    fn hint(&self, line: &str, pos: usize, ctx: &Context<'_>) -> Option<Suggestion> {
        if pos < line.len() || line.trim().is_empty() {
            return None;
        }
        if self.detector.classify(line) == Kind::Natural {
            return Some(Suggestion::Ai);
        }
        if line.chars().count() < 2 {
            return None;
        }
        let h = ctx.history();
        let newest_first = (0..h.len()).rev().filter_map(|i| h.get(i, SearchDirection::Reverse).ok().flatten());
        ghost(line, newest_first.map(|r| r.entry)).map(Suggestion::Ghost)
    }
}

impl Highlighter for Input {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(&'s self, prompt: &'p str, _default: bool) -> Cow<'b, str> {
        if prompt == AI_PROMPT {
            return Cow::Owned(format!("  {ACCENT}{BOLD}{SPARK} run {ARROW}{RESET} "));
        }
        let p = self.prompt.borrow();
        if prompt == p.0 {
            Cow::Owned(p.1.clone())
        } else {
            Cow::Borrowed(prompt)
        }
    }

    /// Text turns the accent colour the moment it reads as English; a known command name is set in bold.
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        if line.trim().is_empty() {
            return Cow::Borrowed(line);
        }
        if self.detector.classify(line) == Kind::Natural {
            return Cow::Owned(format!("{ACCENT}{line}{RESET}"));
        }
        let start = line.len() - line.trim_start().len();
        let end = line[start..].find(char::is_whitespace).map_or(line.len(), |i| start + i);
        if self.detector.is_command(line[start..end].trim_start_matches('!')) {
            Cow::Owned(format!("{}{BOLD}{}{RESET}{}", &line[..start], &line[start..end], &line[end..]))
        } else {
            Cow::Borrowed(line)
        }
    }

    fn highlight_char(&self, _line: &str, _pos: usize, _forced: bool) -> bool {
        true
    }

    /// Only the hint text arrives here, so the AI marker is told apart by its exact text.
    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        let colour = if hint == AI_HINT { ACCENT } else { MUTED };
        Cow::Owned(format!("{colour}{hint}{RESET}"))
    }
}

/// Right, End and Ctrl-E take the grey suggestion. rustyline maps only Right to it, and Right on the
/// AI marker (which has nothing to insert) would beep.
pub fn bind_keys(rl: &mut Editor<Input, DefaultHistory>) {
    for key in [KeyEvent(KeyCode::Right, Modifiers::NONE), KeyEvent(KeyCode::End, Modifiers::NONE), KeyEvent::ctrl('E')] {
        rl.bind_sequence(key, EventHandler::Conditional(Box::new(AcceptGhost)));
    }
}

struct AcceptGhost;
impl ConditionalEventHandler for AcceptGhost {
    fn handle(&self, _: &Event, _: RepeatCount, _: bool, ctx: &EventContext) -> Option<Cmd> {
        if ctx.pos() < ctx.line().len() {
            None
        } else if ctx.hint_text().is_some() {
            Some(Cmd::CompleteHint)
        } else if ctx.has_hint() {
            Some(Cmd::Noop)
        } else {
            None
        }
    }
}

/// Start of the word under the cursor when it is the command name: the first word, after an
/// optional `!`, and not a path.
fn command_word(before: &str) -> Option<usize> {
    let lead = before.len() - before.trim_start().len();
    let start = lead + usize::from(before[lead..].starts_with('!'));
    let word = &before[start..];
    let path = word.contains(['/', '\\']) || word.starts_with(['.', '~']);
    (!word.is_empty() && !word.contains(char::is_whitespace) && !path).then_some(start)
}

/// Start of the word under the cursor when it is likely a branch: `git <sub> … word`, not a flag or path.
fn ref_word(before: &str) -> Option<usize> {
    let start = before.len() - before.rsplit(char::is_whitespace).next().unwrap_or("").len();
    let word = &before[start..];
    if word.starts_with(['-', '.', '/', '~']) || word.contains(['\\', ':']) {
        return None;
    }
    let mut head = before[..start].split_whitespace();
    let git = head.next()?.trim_start_matches('!');
    (git == "git" && GIT_REF_CMDS.contains(&head.next()?)).then_some(start)
}

/// The rest of the newest entry that extends `line`; entries equal to it are skipped.
fn ghost<S: AsRef<str>>(line: &str, newest_first: impl IntoIterator<Item = S>) -> Option<String> {
    newest_first.into_iter().find_map(|e| {
        let e = e.as_ref();
        (e.len() > line.len() && e.starts_with(line)).then(|| e[line.len()..].to_string())
    })
}

/// Local branches, then tags, from loose refs and `packed-refs`. Worktrees keep refs in the common dir.
fn git_refs(git_dir: &Path) -> Vec<String> {
    let common = match std::fs::read_to_string(git_dir.join("commondir")) {
        Ok(c) => git_dir.join(c.trim()),
        Err(_) => git_dir.to_path_buf(),
    };
    let mut full = Vec::new();
    for kind in ["heads", "tags"] {
        walk(&common.join("refs").join(kind), &format!("refs/{kind}/"), &mut full);
    }
    if let Ok(text) = std::fs::read_to_string(common.join("packed-refs")) {
        full.extend(packed_refs(&text).map(str::to_string));
    }
    full.sort(); // refs/heads/ sorts before refs/tags/
    full.dedup();
    full.iter().filter_map(|r| r.strip_prefix("refs/heads/").or_else(|| r.strip_prefix("refs/tags/"))).map(str::to_string).collect()
}

fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = format!("{prefix}{}", e.file_name().to_string_lossy());
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            walk(&e.path(), &format!("{name}/"), out);
        } else {
            out.push(name);
        }
    }
}

/// Branch and tag ref names (`refs/heads/…`, `refs/tags/…`) in a `packed-refs` file.
fn packed_refs(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .filter(|l| !l.starts_with(['#', '^']))
        .filter_map(|l| l.split_once(' ').map(|(_, name)| name.trim()))
        .filter(|n| n.starts_with("refs/heads/") || n.starts_with("refs/tags/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_word() {
        assert_eq!(command_word("gi"), Some(0));
        assert_eq!(command_word("  gi"), Some(2));
        assert_eq!(command_word("!gi"), Some(1));
        for s in ["", "!", "git ", "git st", "./ru", "~/bi", "src/ma", "c:\\x", ".."] {
            assert_eq!(command_word(s), None, "{s}");
        }
    }

    #[test]
    fn ref_context() {
        assert_eq!(ref_word("git checkout "), Some(13));
        assert_eq!(ref_word("git checkout ma"), Some(13));
        assert_eq!(ref_word("!git merge feature/x"), Some(11));
        assert_eq!(ref_word("git log --oneline ma"), Some(18));
        assert_eq!(ref_word("git cherry-pick "), Some(16));
        for s in ["git checkout", "git commit ma", "git checkout -b", "git diff ./sr", "gitk ma", "ls ma", "git"] {
            assert_eq!(ref_word(s), None, "{s}");
        }
    }

    #[test]
    fn packed() {
        let text = "# pack-refs with: peeled fully-peeled sorted\n\
                    1111 refs/heads/main\n\
                    2222 refs/heads/feature/x\n\
                    3333 refs/remotes/origin/main\n\
                    4444 refs/tags/v1.0\n\
                    ^5555\n";
        assert_eq!(packed_refs(text).collect::<Vec<_>>(), ["refs/heads/main", "refs/heads/feature/x", "refs/tags/v1.0"]);
    }

    #[test]
    fn refs_from_disk() {
        let dir = std::env::temp_dir().join(format!("aiterm-refs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("refs/heads/feature")).unwrap();
        std::fs::create_dir_all(dir.join("refs/tags")).unwrap();
        std::fs::write(dir.join("refs/heads/main"), "1").unwrap();
        std::fs::write(dir.join("refs/heads/feature/x"), "1").unwrap();
        std::fs::write(dir.join("refs/tags/v2"), "1").unwrap();
        std::fs::write(dir.join("packed-refs"), "1 refs/heads/main\n1 refs/heads/old\n1 refs/tags/v1\n").unwrap();
        assert_eq!(git_refs(&dir), ["feature/x", "main", "old", "v1", "v2"]);
        // a worktree's git dir points back at the shared one
        let wt = dir.join("worktrees/wt");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join("commondir"), "../..\n").unwrap();
        assert_eq!(git_refs(&wt), ["feature/x", "main", "old", "v1", "v2"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn history_ghost() {
        let newest_first = ["git status", "git stash pop", "git st", "cargo build"];
        assert_eq!(ghost("git st", newest_first).as_deref(), Some("atus"));
        assert_eq!(ghost("git sta", ["git stash pop", "git status"]).as_deref(), Some("sh pop"));
        assert_eq!(ghost("git st", ["git st", "git stash"]).as_deref(), Some("ash")); // equal entry skipped
        assert_eq!(ghost("cargo build", newest_first), None);
        assert_eq!(ghost("ls", newest_first), None);
    }
}
