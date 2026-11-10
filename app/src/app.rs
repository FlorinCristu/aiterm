//! The window: one event loop owning the webview and the tabs; everything else (PTY readers,
//! the side-channel pipe, the page) talks to it through `Ev`.
//!
//! Page → host (`window.ipc.postMessage`, tab-separated, free text last):
//!   ready · spawn id cols rows · i id data · r id cols rows · a id (chunk written) · new id ·
//!   close id · win id · active id · copy text · paste id · font px · bell
//! Host → page: `T.add/out/remove/paste/theme(...)` via `evaluate_script`.
//! aiterm.exe → host (pipe, one line each): cwd/tab/window \t tab \t path, theme \t tab.

use crate::pty::Tab;
use crate::{theme, win};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::time::Duration;
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::windows::{IconExtWindows, WindowExtWindows};
use tao::window::{Icon, Theme as TitleBar, UserAttentionType, Window, WindowBuilder};
use wry::http::{header::CONTENT_TYPE, Request, Response};
use wry::{DragDropEvent, WebContext, WebView, WebViewBuilder, WebViewBuilderExtWindows};

/// Most bytes sent to the page in one go; the next chunk waits for xterm.js to finish this one.
const CHUNK: usize = 256 * 1024;
const FONT_DEFAULT: u32 = 16; // same default and range as src/font.rs
const FONT_RANGE: std::ops::RangeInclusive<u32> = 10..=48;

const INDEX: &str = include_str!("../assets/index.html");
const APP_JS: &str = include_str!("../assets/app.js");
const XTERM_JS: &str = include_str!("../assets/xterm.js");
const XTERM_CSS: &str = include_str!("../assets/xterm.css");
const FIT_JS: &str = include_str!("../assets/addon-fit.js");
const WEBGL_JS: &str = include_str!("../assets/addon-webgl.js");

enum Ev {
    Ipc(String),
    Pipe(String),
    Flush,
    Exited(u32),
    Dropped(Vec<PathBuf>),
    Show,
}

/// Coalesces PTY output wake-ups: the first byte flushes at once, then at most every 10 ms.
struct Wake {
    pending: AtomicBool,
    tx: Sender<()>,
}

impl Wake {
    fn new(proxy: EventLoopProxy<Ev>) -> Arc<Wake> {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for () in rx {
                if proxy.send_event(Ev::Flush).is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        Arc::new(Wake { pending: AtomicBool::new(false), tx })
    }

    fn poke(&self) {
        if !self.pending.swap(true, Ordering::AcqRel) {
            let _ = self.tx.send(());
        }
    }
}

struct App {
    window: Window,
    webview: WebView,
    proxy: EventLoopProxy<Ev>,
    wake: Arc<Wake>,
    tabs: HashMap<u32, Tab>,
    /// Tabs the page is still creating, with the folder they start in.
    pending: HashMap<u32, PathBuf>,
    next: u32,
    active: u32,
    pipe: String,
    shown: bool,
}

pub fn run() {
    if wry::webview_version().is_err() {
        win::message_box(
            "aiterm's window needs the Microsoft Edge WebView2 Runtime, which isn't installed on this PC.\n\n\
             Get it from https://developer.microsoft.com/microsoft-edge/webview2/ and start aiterm again, \
             or run aiterm.exe in a terminal instead.",
        );
        return;
    }
    let font = saved_font();
    let theme = theme::load();
    let start = start_dir();
    let (r, g, b) = theme.bg;

    let event_loop = EventLoopBuilder::<Ev>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    // Cascadia Mono cells are about 0.55 × 1.2 of the font size; plus padding, scrollbar, tab bar.
    let (cw, ch) = ((font as f64 * 0.55).ceil(), (font as f64 * 1.2).ceil());
    // Hidden until the page has painted its first frame: no white flash.
    let window = match WindowBuilder::new()
        .with_title("aiterm")
        .with_visible(false)
        .with_inner_size(LogicalSize::new(110.0 * cw + 30.0, 30.0 * ch + 44.0))
        .with_min_inner_size(LogicalSize::new(360.0, 200.0))
        .with_theme(Some(if theme.dark() { TitleBar::Dark } else { TitleBar::Light }))
        .with_window_icon(Icon::from_resource(1, None).ok())
        .with_background_color((r, g, b, 255))
        .build(&event_loop)
    {
        Ok(w) => w,
        Err(e) => return win::message_box(&format!("Couldn't open a window: {e}")),
    };

    let pipe = format!(r"\\.\pipe\aiterm-app-{}", std::process::id());
    {
        let (name, p) = (pipe.clone(), proxy.clone());
        std::thread::spawn(move || win::serve_pipe(&name, move |line| drop(p.send_event(Ev::Pipe(line)))));
    }

    let boot = format!(
        "window.BOOT={{theme:{},font:{font},first:1,build:{}}};",
        theme.json(),
        win::build_number()
    );
    // Not next to the exe: that folder may be read-only (Program Files).
    let data_dir = std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("aiterm").join("WebView2"));
    let mut context = WebContext::new(data_dir);
    let built = WebViewBuilder::new_with_web_context(&mut context)
        .with_url("aiterm://localhost/")
        .with_custom_protocol("aiterm".into(), move |_, req: Request<Vec<u8>>| serve(req.uri().path(), &boot))
        .with_ipc_handler({
            let p = proxy.clone();
            move |req: Request<String>| drop(p.send_event(Ev::Ipc(req.into_body())))
        })
        .with_drag_drop_handler({
            let p = proxy.clone();
            move |e| {
                if let DragDropEvent::Drop { paths, .. } = e {
                    let _ = p.send_event(Ev::Dropped(paths));
                }
                true // never let the webview navigate to a dropped file
            }
        })
        .with_navigation_handler(|url| url.starts_with("http://aiterm.localhost/") || url.starts_with("aiterm:"))
        .with_background_color((r, g, b, 255))
        .with_devtools(cfg!(debug_assertions))
        .with_hotkeys_zoom(false)
        .with_back_forward_navigation_gestures(false)
        .with_browser_accelerator_keys(false)
        .with_default_context_menus(false)
        .with_focused(true)
        .build(&window);
    let webview = match built {
        Ok(w) => w,
        Err(e) => return win::message_box(&format!("Couldn't start the WebView2 runtime: {e}")),
    };

    let wake = Wake::new(proxy.clone());
    let mut app = App {
        window,
        webview,
        proxy: proxy.clone(),
        wake,
        tabs: HashMap::new(),
        pending: HashMap::from([(1, start)]),
        next: 2,
        active: 1,
        pipe,
        shown: false,
    };
    // Should the page never say it's ready, show the window anyway.
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(3));
        let _ = proxy.send_event(Ev::Show);
    });

    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        let _ = &context; // the webview's environment lives as long as the loop
        match event {
            Event::UserEvent(ev) => {
                if !app.handle(ev) {
                    app.quit();
                }
            }
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => app.quit(),
            Event::WindowEvent { event: WindowEvent::Focused(true), .. } => {
                let _ = app.webview.focus();
            }
            _ => {}
        }
    });
}

fn serve(path: &str, boot: &str) -> Response<Cow<'static, [u8]>> {
    let (mime, body): (&str, Cow<'static, [u8]>) = match path {
        "/" | "/index.html" => ("text/html", INDEX.replace("/*BOOT*/", boot).into_bytes().into()),
        "/app.js" => ("text/javascript", APP_JS.as_bytes().into()),
        "/xterm.js" => ("text/javascript", XTERM_JS.as_bytes().into()),
        "/addon-fit.js" => ("text/javascript", FIT_JS.as_bytes().into()),
        "/addon-webgl.js" => ("text/javascript", WEBGL_JS.as_bytes().into()),
        "/xterm.css" => ("text/css", XTERM_CSS.as_bytes().into()),
        _ => return Response::builder().status(404).body(Cow::Borrowed(&[][..])).unwrap(),
    };
    Response::builder().header(CONTENT_TYPE, mime).body(body).unwrap()
}

impl App {
    /// False when the window should close.
    fn handle(&mut self, ev: Ev) -> bool {
        match ev {
            Ev::Ipc(msg) => self.ipc(&msg),
            Ev::Pipe(line) => self.pipe_msg(&line),
            Ev::Flush => self.flush(),
            Ev::Exited(id) => {
                self.tabs.remove(&id);
                self.eval(&format!("T.remove({id})"));
                return !(self.tabs.is_empty() && self.pending.is_empty());
            }
            Ev::Dropped(paths) => {
                // Single quotes keep backslashes and spaces intact in bash.
                let quoted: Vec<String> =
                    paths.iter().map(|p| format!("'{}'", p.display().to_string().replace('\'', r"'\''"))).collect();
                if let Some(t) = self.tabs.get(&self.active) {
                    t.write(quoted.join(" ").as_bytes());
                }
            }
            Ev::Show => self.show(),
        }
        true
    }

    fn ipc(&mut self, msg: &str) {
        let (op, rest) = msg.split_once('\t').unwrap_or((msg, ""));
        let mut f = rest.splitn(3, '\t');
        let id: u32 = f.clone().next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let num = |s: Option<&str>| s.and_then(|s| s.parse::<u16>().ok()).unwrap_or(0);
        match op {
            "ready" => self.show(),
            "i" => {
                if let (Some(t), Some((_, data))) = (self.tabs.get(&id), rest.split_once('\t')) {
                    t.write(data.as_bytes());
                }
            }
            "spawn" => {
                f.next();
                let (cols, rows) = (num(f.next()), num(f.next()));
                self.spawn(id, cols, rows);
            }
            "r" => {
                f.next();
                let (cols, rows) = (num(f.next()), num(f.next()));
                if let Some(t) = self.tabs.get(&id) {
                    t.resize(cols, rows);
                }
            }
            "a" => {
                if let Some(t) = self.tabs.get_mut(&id) {
                    t.in_flight = false;
                }
                self.flush();
            }
            "new" => {
                let cwd = self.cwd_of(id);
                self.new_tab(cwd);
            }
            "win" => new_window(&self.cwd_of(id)),
            "close" => match self.tabs.get_mut(&id) {
                Some(t) if t.has_child() => t.kill(), // the waiter thread then reports Exited
                _ => drop(self.proxy.send_event(Ev::Exited(id))),
            },
            "active" => self.active = id,
            "copy" => win::set_clipboard(self.window.hwnd(), rest),
            "paste" => {
                if let Some(text) = win::clipboard(self.window.hwnd()) {
                    self.eval(&format!("T.paste({id},{})", js_str(&text)));
                }
            }
            "font" => {
                if let (Ok(px), Some(home)) = (rest.parse::<u32>(), home()) {
                    let _ = std::fs::write(home.join(".aiterm_font"), px.clamp(*FONT_RANGE.start(), *FONT_RANGE.end()).to_string());
                }
            }
            "bell" => self.window.request_user_attention(Some(UserAttentionType::Informational)),
            _ => {}
        }
    }

    fn pipe_msg(&mut self, line: &str) {
        let mut f = line.splitn(3, '\t');
        let (kind, tab, path) = (f.next().unwrap_or(""), f.next().and_then(|s| s.parse::<u32>().ok()), f.next());
        let path = path.map(PathBuf::from).filter(|p| p.is_dir());
        match (kind, path) {
            ("cwd", Some(p)) => {
                if let Some(t) = tab.and_then(|id| self.tabs.get_mut(&id)) {
                    t.cwd = p;
                }
            }
            ("tab", Some(p)) => self.new_tab(p),
            ("window", Some(p)) => new_window(&p),
            ("theme", _) => {
                let t = theme::load();
                let (r, g, b) = t.bg;
                self.window.set_theme(Some(if t.dark() { TitleBar::Dark } else { TitleBar::Light }));
                let _ = self.webview.set_background_color((r, g, b, 255));
                self.eval(&format!("T.theme({})", t.json()));
            }
            _ => {}
        }
    }

    fn cwd_of(&self, id: u32) -> PathBuf {
        self.tabs.get(&id).map(|t| t.cwd.clone()).or_else(home).unwrap_or_else(|| PathBuf::from("C:\\"))
    }

    fn new_tab(&mut self, cwd: PathBuf) {
        let id = self.next;
        self.next += 1;
        self.pending.insert(id, cwd);
        self.eval(&format!("T.add({id})"));
    }

    /// The page made tab `id` and measured it; start its aiterm.
    fn spawn(&mut self, id: u32, cols: u16, rows: u16) {
        let Some(cwd) = self.pending.remove(&id) else { return };
        let tab = match find_aiterm() {
            None => Err("aiterm.exe was not found next to aiterm-app.exe or on PATH.".to_string()),
            Some(exe) => {
                let wake = self.wake.clone();
                let proxy = self.proxy.clone();
                let env = [("AITERM_APP_PIPE", self.pipe.clone()), ("AITERM_APP_TAB", id.to_string())];
                Tab::spawn(&exe, cwd.clone(), (cols, rows), &env, move || wake.poke(), move || drop(proxy.send_event(Ev::Exited(id))))
            }
        };
        let tab = tab.unwrap_or_else(|e| Tab::empty(cwd, &format!("\r\n  \x1b[31m♦ couldn't start aiterm: {e}\x1b[0m\r\n  \x1b[90mCtrl+Shift+W closes this tab.\x1b[0m\r\n")));
        self.tabs.insert(id, tab);
        self.wake.poke();
    }

    /// Hand buffered output to the page, one chunk in flight per tab.
    fn flush(&mut self) {
        self.wake.pending.store(false, Ordering::Release);
        let mut js = String::new();
        for (id, t) in &mut self.tabs {
            if t.in_flight {
                continue;
            }
            let chunk = t.out.take(CHUNK);
            if !chunk.is_empty() {
                t.in_flight = true;
                let _ = write!(js, "T.out({id},'{}');", b64(&chunk));
            }
        }
        if !js.is_empty() {
            self.eval(&js);
        }
    }

    fn show(&mut self) {
        if !self.shown {
            self.shown = true;
            self.window.set_visible(true);
            self.window.set_focus();
            let _ = self.webview.focus();
        }
    }

    /// Kill the children and leave at once. Exiting the process (rather than the loop) also
    /// works when something closed tao's hidden helper window (`taskkill` sends WM_CLOSE to all).
    fn quit(&mut self) -> ! {
        self.window.set_visible(false);
        for (_, t) in self.tabs.drain() {
            t.close();
        }
        std::process::exit(0)
    }

    fn eval(&self, js: &str) {
        let _ = self.webview.evaluate_script(js);
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(PathBuf::from)
}

fn saved_font() -> u32 {
    home()
        .and_then(|h| std::fs::read_to_string(h.join(".aiterm_font")).ok())
        .and_then(|s| s.trim().parse::<u32>().ok())
        .map(|s| s.clamp(*FONT_RANGE.start(), *FONT_RANGE.end()))
        .unwrap_or(FONT_DEFAULT)
}

/// A folder given on the command line, else where we were started, unless that is just the
/// install folder or System32 (Start Menu, Explorer): then home.
fn start_dir() -> PathBuf {
    if let Some(p) = std::env::args_os().nth(1).map(PathBuf::from).filter(|p| p.is_dir()) {
        return p;
    }
    let cwd = std::env::current_dir().ok();
    let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
    let windir = std::env::var_os("WINDIR").map(PathBuf::from);
    match cwd {
        Some(c) if Some(&c) != exe_dir.as_ref() && !windir.is_some_and(|w| c.starts_with(w)) => c,
        _ => home().unwrap_or_else(|| PathBuf::from("C:\\")),
    }
}

fn find_aiterm() -> Option<PathBuf> {
    let beside = std::env::current_exe().ok().map(|e| e.with_file_name("aiterm.exe")).filter(|p| p.is_file());
    beside.or_else(|| {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path).map(|d| d.join("aiterm.exe")).find(|p| p.is_file())
    })
}

fn new_window(dir: &Path) {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).arg(dir).current_dir(dir).spawn();
    }
}

fn b64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            s.push(if i <= c.len() { T[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    s
}

/// A JavaScript string literal for arbitrary text (clipboard contents).
fn js_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            c if (c as u32) < 0x20 || c == '\u{2028}' || c == '\u{2029}' => {
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes() {
        assert_eq!(b64(b""), "");
        assert_eq!(b64(b"f"), "Zg==");
        assert_eq!(b64(b"fo"), "Zm8=");
        assert_eq!(b64(b"foo"), "Zm9v");
        assert_eq!(b64(&[0xe2, 0x99, 0xa6, 0xff]), "4pmm/w==");
        assert_eq!(js_str("a\"b\\c\n\u{1b}\u{2028}"), r#""a\"b\\c\n\u001b\u2028""#);
    }
}
