# aiterm

A small, fast bash front-end written in Rust. Type a command and it runs in bash. Type plain English and DeepSeek (or Claude or Codex, on your own subscription) turns it into a command, which you can edit before pressing Enter to run it.

```
~/code ❯ git status                         → runs in bash
~/code ❯ find all files bigger than 100mb   ✦ AI
✦ 0.41s · Enter to run, edit first if you like, Ctrl-C to cancel
✦ run ❯ find . -type f -size +100M
```

## Setup

```bash
setx DEEPSEEK_API_KEY sk-...     # or put the key in ~/.aiterm_key
cargo build --release
```

| Env var | Default | Purpose |
|---|---|---|
| `DEEPSEEK_API_KEY` | – | API key (or the file `~/.aiterm_key`) |
| `DEEPSEEK_MODEL` | `deepseek-v4-flash` | Model ID |
| `DEEPSEEK_BASE_URL` | `https://api.deepseek.com` | Any OpenAI-compatible endpoint |
| `DEEPSEEK_THINKING` | off | Set to `1` to allow reasoning (slower) |
| `AITERM_BASH` | Git Bash on Windows, `bash` elsewhere | Which bash to run |
| `AITERM_BACKEND` | saved choice, else `deepseek` | `deepseek`, `claude` or `codex` |
| `AITERM_CLAUDE_MODEL` | `haiku` | Model for the Claude backend |
| `AITERM_CODEX_MODEL` | Codex's default | Model for the Codex backend |

### Use your Claude or ChatGPT subscription instead

Type `/`, choose **1 AI**, then pick **Claude** or **Codex**. aiterm runs the official CLI headless (`claude -p` or `codex exec`), so your requests use the plan you signed in to through that CLI's browser login. aiterm never handles your password or token. The menu can start the sign-in for you (`claude auth login` / `codex login`), and the choice is saved in `~/.aiterm_backend`.

You need the CLI on PATH (`npm install -g @anthropic-ai/claude-code` or `npm install -g @openai/codex`). This is slower than DeepSeek: each request starts the CLI, so a reply takes a few seconds (about 5s with Claude Haiku) rather than half a second.

## Install on Windows (Start Menu + PATH, no admin)

```bash
powershell -ExecutionPolicy Bypass -File packaging\build.ps1
```

This builds `dist\aiterm-setup\` and `dist\aiterm-setup.zip`. To install, run `install.cmd` from there. aiterm then appears in Windows search and in Settings → Apps, and you can uninstall it from there.

The Start Menu shortcut opens aiterm in its own console window, so the taskbar shows the aiterm icon. To open it in Windows Terminal instead, install with `install.ps1 -UseWindowsTerminal`. Windows Terminal renders text more nicely, but the taskbar then shows the Terminal icon.

`dev/test-server.js` is a tiny HTTP server on port 3300. It's handy for testing requests like "kill whatever is on port 3300".

## Keys

- `#text`: always ask the AI. `!text`: always run in bash.
- While you type, a dim `✦ AI` hint shows when Enter will go to the AI instead of bash.
- Tab completes file names. Up and Down browse history, saved in `~/.aiterm_history`.
- Ctrl-C stops the running command or cancels an AI request. Ctrl-D quits.

## How it decides (`src/detect.rs`)

This follows the approach of Warp's `is_likely_shell_command` heuristic:

1. If the line contains shell operators (`| && ; > $( \``), a path, or a `VAR=` assignment, it goes to bash.
2. If the first word is not a known command (bash built-ins plus everything on PATH, including Git's `usr/bin`), it goes to the AI.
3. If the first word is a known command, aiterm counts English filler words (`the`, `all`, `my`, `than`, …). If enough of them appear, it goes to the AI. Words inside quotes are ignored, so `git commit -m "fix the bug"` stays a command.

## Speed

- The release binary is about 2 MB and starts instantly.
- The TLS connection to DeepSeek opens in the background at startup, so the first request skips the handshake.
- Thinking is off, temperature is 0, and the reply is streamed and capped at 300 tokens.
- Each command runs as a direct `bash -c` with no login-shell startup. `cd` and `export` are handled in-process so their effect persists.

## License

MIT. See [LICENSE](LICENSE).
