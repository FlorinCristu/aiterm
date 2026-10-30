//! Decides whether a line of input is a shell command or a natural-language request.
//!
//! Same idea as Warp's `natural_language_detection` crate (`is_likely_shell_command`):
//! look at the first token, look for shell syntax, and count tokens that read like English.

use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Shell,
    Natural,
}

const BASH_BUILTINS: &[&str] = &[
    ".", ":", "[", "[[", "alias", "bg", "bind", "break", "builtin", "case", "cd", "command",
    "compgen", "complete", "continue", "declare", "dirs", "disown", "echo", "enable", "eval",
    "exec", "exit", "export", "false", "fc", "fg", "for", "function", "getopts", "hash", "help",
    "history", "if", "jobs", "kill", "let", "local", "logout", "popd", "printf", "pushd", "pwd",
    "read", "readonly", "return", "select", "set", "shift", "shopt", "source", "suspend", "test",
    "time", "times", "trap", "true", "type", "typeset", "ulimit", "umask", "unalias", "unset",
    "until", "wait", "while",
];

/// One of these after a command name almost always means the user is talking, not typing a command.
const STRONG_NL: &[&str] = &[
    "the", "a", "an", "all", "every", "my", "me", "i", "than", "that", "which", "please", "how",
    "what", "where", "why", "who", "is", "are", "can", "you", "should", "would", "could", "it",
];

/// Weaker hints: two of these are needed.
const WEAK_NL: &[&str] = &[
    "files", "file", "folder", "folders", "directory", "directories", "by", "size", "in", "to",
    "from", "of", "with", "into", "about", "bigger", "larger", "smaller", "last", "recent",
    "older", "newer", "each", "and", "or", "for", "on", "lines", "contain", "containing",
];

pub struct Detector {
    commands: HashSet<String>,
}

impl Detector {
    pub fn new(extra_dirs: &[std::path::PathBuf]) -> Self {
        let mut commands: HashSet<String> = BASH_BUILTINS.iter().map(|s| s.to_string()).collect();
        let mut dirs: Vec<std::path::PathBuf> =
            std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
        dirs.extend_from_slice(extra_dirs);
        for dir in dirs {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for e in entries.flatten() {
                let name = e.file_name();
                let Some(name) = name.to_str() else { continue };
                if let Some(cmd) = command_name(name) {
                    commands.insert(cmd);
                }
            }
        }
        Detector { commands }
    }

    pub fn is_command(&self, word: &str) -> bool {
        let w = if cfg!(windows) { word.to_ascii_lowercase() } else { word.to_string() };
        self.commands.contains(&w)
    }

    pub fn classify(&self, input: &str) -> Kind {
        let line = input.trim();
        if line.starts_with('#') {
            return Kind::Natural;
        }
        if line.starts_with('!') {
            return Kind::Shell;
        }
        if line.ends_with('?') {
            return Kind::Natural;
        }
        // Shell operators are a near-certain signal. Apostrophes ("what's") are not.
        for op in ["|", "&&", "||", ";", ">", "<", "$(", "`", "${"] {
            if line.contains(op) {
                return Kind::Shell;
            }
        }

        let unquoted = strip_quoted(line);
        let tokens: Vec<&str> = unquoted.split_whitespace().collect();
        let Some(&first) = tokens.first() else { return Kind::Shell };

        if first.contains('/') || first.contains('\\') || first.starts_with('.') || first.starts_with('~')
            || first.starts_with('$') || (first.contains('=') && !first.starts_with('='))
        {
            return Kind::Shell;
        }
        if !self.is_command(first) {
            return Kind::Natural;
        }
        if tokens.len() == 1 || tokens[1..].iter().any(|t| t.starts_with('-')) {
            return Kind::Shell;
        }

        let (mut strong, mut weak) = (0, 0);
        for t in &tokens[1..] {
            let w = t.trim_matches(|c: char| c == ',' || c == '.').to_ascii_lowercase();
            if !w.chars().all(|c| c.is_ascii_alphabetic() || c == '\'') || Path::new(t).exists() {
                continue;
            }
            if STRONG_NL.contains(&w.as_str()) {
                strong += 1;
            } else if WEAK_NL.contains(&w.as_str()) {
                weak += 1;
            }
        }
        if strong >= 1 || weak >= 2 { Kind::Natural } else { Kind::Shell }
    }
}

fn command_name(file: &str) -> Option<String> {
    if cfg!(windows) {
        let lower = file.to_ascii_lowercase();
        for ext in [".exe", ".cmd", ".bat", ".com"] {
            if let Some(stem) = lower.strip_suffix(ext) {
                return Some(stem.to_string());
            }
        }
        // MSYS/Git-bash ships extension-less shell scripts too
        if !lower.contains('.') { Some(lower) } else { None }
    } else {
        Some(file.to_string())
    }
}

/// Drop text inside quotes that start a token, so `git commit -m "fix the bug"` doesn't look like English.
fn strip_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    let mut at_token_start = true;
    while let Some(c) = chars.next() {
        if at_token_start && (c == '"' || c == '\'') {
            for d in chars.by_ref() {
                if d == c {
                    break;
                }
            }
            out.push_str(" Q ");
            at_token_start = false;
            continue;
        }
        at_token_start = c.is_whitespace();
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn det() -> Detector {
        let mut d = Detector { commands: HashSet::new() };
        for c in BASH_BUILTINS.iter().chain(["ls", "git", "find", "cat", "grep", "make", "where", "sort", "du"].iter()) {
            d.commands.insert(c.to_string());
        }
        d
    }

    #[test]
    fn commands() {
        let d = det();
        for s in ["ls", "ls -la", "git status", "git commit -m \"fix the bug\"", "echo hello world",
                  "cat foo.txt | grep bar", "./run.sh", "X=1 make", "du -sh *", "!find all"] {
            assert_eq!(d.classify(s), Kind::Shell, "{s}");
        }
    }

    #[test]
    fn natural() {
        let d = det();
        for s in ["find all files bigger than 100mb", "show me the largest folders here",
                  "where is my ssh config", "how much disk space is free?", "list running processes",
                  "make me a sandwich", "sort files by size", "#ls"] {
            assert_eq!(d.classify(s), Kind::Natural, "{s}");
        }
    }
}
