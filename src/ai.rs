//! DeepSeek client (OpenAI-compatible chat completions, streamed over SSE), and the backend
//! switch that can send requests to the Claude Code or Codex CLI instead.

use crate::cli::{self, Cli};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub struct Client {
    agent: ureq::Agent,
    key: String,
    model: String,
    base: String,
    disable_thinking: bool,
}

pub struct Context<'a> {
    pub cwd: &'a str,
    pub os: &'a str,
    pub files: &'a [String],
    pub last: Option<(&'a str, i32)>,
}

/// Who answers: DeepSeek over HTTP with an API key, or a subscription CLI (Claude Code, Codex).
pub enum Backend {
    DeepSeek(Client),
    Cli(Cli),
}

impl Backend {
    /// AITERM_BACKEND, else the choice saved in ~/.aiterm_backend, else DeepSeek.
    /// None when the chosen backend isn't usable yet (no key, CLI not installed).
    pub fn load() -> Option<Self> {
        match chosen() {
            Some(kind) => Cli::find(kind).map(Backend::Cli),
            None => Client::from_env().map(Backend::DeepSeek),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Backend::DeepSeek(c) => c.model().to_string(),
            Backend::Cli(c) => c.label(),
        }
    }

    pub fn warm_up(&self) {
        if let Backend::DeepSeek(c) = self {
            c.warm_up();
        }
    }

    pub fn suggest(
        &self,
        request: &str,
        ctx: &Context,
        cancel: &AtomicBool,
        on_token: impl FnMut(&str),
    ) -> Result<String, String> {
        match self {
            Backend::DeepSeek(c) => c.suggest(request, ctx, cancel, on_token),
            Backend::Cli(c) => command(&c.ask(&flatten(&suggest_messages(request, ctx)), cancel)?),
        }
    }

    /// Like `suggest`, but the model already proposed `previous` for `request`; `feedback` says what to change.
    /// Returns the corrected single command.
    pub fn revise(
        &self,
        request: &str,
        previous: &str,
        feedback: &str,
        ctx: &Context,
        cancel: &AtomicBool,
        on_token: impl FnMut(&str),
    ) -> Result<String, String> {
        let messages = revise_messages(request, previous, feedback, ctx);
        command(&match self {
            Backend::DeepSeek(c) => c.chat(messages, 300, cancel, on_token)?,
            Backend::Cli(c) => c.ask(&flatten(&messages), cancel)?,
        })
    }

    /// A short plain-English explanation of what `command` does (what it touches, anything risky). Not a command.
    pub fn explain(
        &self,
        command: &str,
        ctx: &Context,
        cancel: &AtomicBool,
        on_token: impl FnMut(&str),
    ) -> Result<String, String> {
        let text = match self {
            Backend::DeepSeek(c) => c.chat(explain_messages(command, ctx), 200, cancel, on_token)?.trim().to_string(),
            // No streaming here, so the explanation is tidied before anyone sees it.
            Backend::Cli(c) => plain(&c.ask(&format!("{}\nCommand: {command}\n", explain_prompt(ctx)), cancel)?),
        };
        if text.is_empty() { Err("model returned no explanation".into()) } else { Ok(text) }
    }
}

/// The saved CLI choice; None means DeepSeek.
pub fn chosen() -> Option<cli::Kind> {
    let saved = std::env::var("AITERM_BACKEND")
        .ok()
        .or_else(|| std::fs::read_to_string(home_dir()?.join(".aiterm_backend")).ok())?;
    match saved.trim().to_lowercase().as_str() {
        "claude" => Some(cli::Kind::Claude),
        "codex" => Some(cli::Kind::Codex),
        _ => None,
    }
}

pub fn save_choice(kind: Option<cli::Kind>) -> std::io::Result<()> {
    let path = home_dir().ok_or_else(|| std::io::Error::other("no home directory"))?.join(".aiterm_backend");
    std::fs::write(path, kind.map_or("deepseek", cli::Kind::name))
}

impl Client {
    /// Reads DEEPSEEK_API_KEY (or ~/.aiterm_key), DEEPSEEK_MODEL, DEEPSEEK_BASE_URL, DEEPSEEK_THINKING.
    pub fn from_env() -> Option<Self> {
        env_key().or_else(file_key).map(Client::with_key)
    }

    /// A client for `key`; model, base URL and thinking still come from the environment.
    pub fn with_key(key: String) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(5))
            .timeout_read(Duration::from_secs(60))
            .max_idle_connections_per_host(2)
            .build();
        Client {
            agent,
            key,
            model: std::env::var("DEEPSEEK_MODEL").unwrap_or_else(|_| "deepseek-v4-flash".into()),
            base: std::env::var("DEEPSEEK_BASE_URL")
                .unwrap_or_else(|_| "https://api.deepseek.com".into())
                .trim_end_matches('/')
                .to_string(),
            disable_thinking: std::env::var("DEEPSEEK_THINKING").map(|v| v != "1").unwrap_or(true),
        }
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    /// The key with all but its ends hidden: `sk-…a1b2`.
    pub fn masked_key(&self) -> String {
        mask(&self.key)
    }

    /// One cheap authenticated request, to tell a bad key from a good one before saving it.
    pub fn check(&self) -> Result<(), String> {
        match self.agent.get(&format!("{}/models", self.base)).set("Authorization", &format!("Bearer {}", self.key)).call() {
            Ok(r) => {
                let _ = r.into_string();
                Ok(())
            }
            Err(ureq::Error::Status(401 | 403, _)) => Err("DeepSeek rejected that key".into()),
            Err(ureq::Error::Status(code, _)) => Err(format!("DeepSeek answered HTTP {code}")),
            Err(e) => Err(format!("could not reach DeepSeek ({e})")),
        }
    }

    /// Open the TLS connection in the background so the first real request skips the handshake.
    pub fn warm_up(&self) {
        let agent = self.agent.clone();
        let url = format!("{}/models", self.base);
        let auth = format!("Bearer {}", self.key);
        std::thread::spawn(move || {
            if let Ok(r) = agent.get(&url).set("Authorization", &auth).call() {
                let _ = r.into_string();
            }
        });
    }

    /// Ask for one bash command. `on_token` sees streamed text; `cancel` aborts mid-stream.
    pub fn suggest(
        &self,
        request: &str,
        ctx: &Context,
        cancel: &AtomicBool,
        on_token: impl FnMut(&str),
    ) -> Result<String, String> {
        command(&self.chat(suggest_messages(request, ctx), 300, cancel, on_token)?)
    }

    /// One streamed chat completion; returns the raw text. `cancel` aborts mid-stream.
    fn chat(
        &self,
        messages: Value,
        max_tokens: u32,
        cancel: &AtomicBool,
        mut on_token: impl FnMut(&str),
    ) -> Result<String, String> {
        let mut body = json!({
            "model": self.model,
            "stream": true,
            "temperature": 0,
            "max_tokens": max_tokens,
            "messages": messages,
        });
        if self.disable_thinking {
            body["thinking"] = json!({"type": "disabled"});
        }

        let resp = match self.post(&body) {
            // Older/other endpoints may reject the `thinking` field: retry once without it.
            Err(ureq::Error::Status(400, _)) if self.disable_thinking => {
                body.as_object_mut().unwrap().remove("thinking");
                self.post(&body)
            }
            r => r,
        };
        let resp = resp.map_err(|e| match e {
            ureq::Error::Status(code, r) => {
                let msg = r.into_string().unwrap_or_default();
                let detail = serde_json::from_str::<Value>(&msg)
                    .ok()
                    .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
                    .unwrap_or(msg);
                format!("DeepSeek HTTP {code}: {detail}")
            }
            other => format!("network error: {other}"),
        })?;

        let mut out = String::new();
        for line in BufReader::new(resp.into_reader()).lines() {
            if cancel.load(Ordering::Relaxed) {
                return Err("cancelled".into());
            }
            let line = line.map_err(|e| e.to_string())?;
            let Some(data) = line.strip_prefix("data:") else { continue };
            let data = data.trim();
            if data == "[DONE]" {
                break;
            }
            let Ok(v) = serde_json::from_str::<Value>(data) else { continue };
            if let Some(t) = v["choices"][0]["delta"]["content"].as_str() {
                out.push_str(t);
                on_token(t);
            }
        }
        Ok(out)
    }

    fn post(&self, body: &Value) -> Result<ureq::Response, ureq::Error> {
        self.agent
            .post(&format!("{}/chat/completions", self.base))
            .set("Authorization", &format!("Bearer {}", self.key))
            .set("Accept", "text/event-stream")
            .send_json(body)
    }
}

fn system_prompt(ctx: &Context) -> String {
    let mut s = format!(
        "You turn a user's natural-language request into ONE bash command.\n\
         Environment: {os}. Shell: bash. Current directory: {cwd}\n\
         Rules:\n\
         - Reply with the command only: no explanation, no markdown, no code fences, no leading $.\n\
         - A single line. Chain with && or pipes when needed.\n\
         - Prefer common, safe, non-destructive tools and flags; never add sudo unless asked.\n\
         - If the request is ambiguous, pick the most common sensible interpretation.\n",
        os = ctx.os,
        cwd = ctx.cwd
    );
    if !ctx.files.is_empty() {
        s.push_str("Files here: ");
        s.push_str(&ctx.files.join(", "));
        s.push('\n');
    }
    if let Some((cmd, code)) = ctx.last {
        s.push_str(&format!("Previous command: `{cmd}` (exit {code})\n"));
    }
    s
}

fn suggest_messages(request: &str, ctx: &Context) -> Value {
    json!([
        {"role": "system", "content": system_prompt(ctx)},
        {"role": "user", "content": request},
    ])
}

/// A real follow-up turn, so the model sees its own answer and the correction to it.
fn revise_messages(request: &str, previous: &str, feedback: &str, ctx: &Context) -> Value {
    json!([
        {"role": "system", "content": system_prompt(ctx)},
        {"role": "user", "content": request},
        {"role": "assistant", "content": previous},
        {"role": "user", "content": format!("Change it: {feedback}\nReply with the full corrected command only.")},
    ])
}

fn explain_messages(command: &str, ctx: &Context) -> Value {
    json!([
        {"role": "system", "content": explain_prompt(ctx)},
        {"role": "user", "content": command},
    ])
}

fn explain_prompt(ctx: &Context) -> String {
    format!(
        "You explain a bash command to the user in plain English.\n\
         Environment: {os}. Shell: bash. Current directory: {cwd}\n\
         Rules:\n\
         - 1 to 3 short lines of plain text: no markdown, no bullets, no code fences, no backticks.\n\
         - Don't restate the command. Say what it does and what it reads or changes; mention flags only where they matter.\n\
         - If it deletes, overwrites, moves or changes permissions on anything, or is otherwise risky, say so plainly.\n\
         - Reply with the explanation only.\n",
        os = ctx.os,
        cwd = ctx.cwd
    )
}

/// The CLIs take one prompt string: the system prompt, then the turns labelled in order.
fn flatten(messages: &Value) -> String {
    let mut s = String::new();
    for m in messages.as_array().into_iter().flatten() {
        let content = m["content"].as_str().unwrap_or_default();
        match m["role"].as_str() {
            Some("system") => s.push_str(&format!("{content}\n")),
            Some("assistant") => s.push_str(&format!("You answered: {content}\n")),
            _ => s.push_str(&format!("Request: {content}\n")),
        }
    }
    s
}

/// The cleaned command, or an error when nothing usable came back.
fn command(raw: &str) -> Result<String, String> {
    let cmd = clean(raw);
    if cmd.is_empty() { Err("model returned no command".into()) } else { Ok(cmd) }
}

/// Prose, lightly unmarked: drop fence lines and backticks, keep the wording.
fn plain(raw: &str) -> String {
    raw.lines().filter(|l| !l.trim_start().starts_with("```")).collect::<Vec<_>>().join("\n").replace('`', "").trim().to_string()
}

/// Strip fences, prompts and stray prose the model may add despite instructions.
fn clean(raw: &str) -> String {
    let lines: Vec<&str> = raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("```"))
        .map(|l| l.strip_prefix("$ ").unwrap_or(l))
        .collect();
    let joined = lines.join(" && ");
    joined.trim_matches('`').trim().to_string()
}

pub fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(Into::into)
}

pub fn key_path() -> Option<std::path::PathBuf> {
    home_dir().map(|h| h.join(".aiterm_key"))
}

/// Key from DEEPSEEK_API_KEY; it wins over the saved file.
pub fn env_key() -> Option<String> {
    std::env::var("DEEPSEEK_API_KEY").ok().map(|k| k.trim().to_string()).filter(|k| !k.is_empty())
}

fn file_key() -> Option<String> {
    std::fs::read_to_string(key_path()?).ok().map(|s| s.trim().to_string()).filter(|k| !k.is_empty())
}

pub fn save_key(key: &str) -> std::io::Result<()> {
    let path = key_path().ok_or_else(|| std::io::Error::other("no home directory"))?;
    std::fs::write(&path, key)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

pub fn remove_key() -> std::io::Result<()> {
    match key_path() {
        Some(p) if p.exists() => std::fs::remove_file(p),
        _ => Ok(()),
    }
}

fn mask(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 8 {
        return "•".repeat(chars.len());
    }
    let head: String = chars[..3].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Context<'static> {
        Context { cwd: "/home/u", os: "Linux", files: &[], last: None }
    }

    #[test]
    fn cleans_fences() {
        assert_eq!(clean("```bash\nls -la\n```"), "ls -la");
        assert_eq!(clean("$ du -sh *"), "du -sh *");
        assert_eq!(clean("`pwd`"), "pwd");
    }

    #[test]
    fn flattens_like_before() {
        // The CLI prompt for a plain suggestion must not change.
        let c = ctx();
        assert_eq!(flatten(&suggest_messages("list files", &c)), format!("{}\nRequest: list files\n", system_prompt(&c)));
    }

    #[test]
    fn revise_is_a_follow_up_turn() {
        let m = revise_messages("find big files", "find . -size +100M", "only .js files", &ctx());
        let roles: Vec<_> = m.as_array().unwrap().iter().map(|t| t["role"].as_str().unwrap()).collect();
        assert_eq!(roles, ["system", "user", "assistant", "user"]);
        assert!(m[3]["content"].as_str().unwrap().contains("only .js files"));
        let flat = flatten(&m);
        assert!(flat.contains("Request: find big files\nYou answered: find . -size +100M\nRequest: Change it: only .js files"));
    }

    #[test]
    fn explain_prompt_has_context() {
        let m = explain_messages("rm -rf build", &ctx());
        let sys = m[0]["content"].as_str().unwrap();
        assert!(sys.contains("Linux") && sys.contains("/home/u"));
        assert_eq!(m[1]["content"], "rm -rf build");
    }

    #[test]
    fn strips_markdown_from_prose() {
        assert_eq!(plain("```\nDeletes `build` for good.\n```"), "Deletes build for good.");
        assert_eq!(plain("  Lists files.\nIncludes hidden ones.  "), "Lists files.\nIncludes hidden ones.");
    }
}
