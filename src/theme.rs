//! Colour themes in the formats terminalcolors.com hands out (Ghostty, Alacritty, Warp, iTerm2),
//! applied to the whole window so bash output follows the theme too.
//!
//! aiterm's own chrome only uses the 16 ANSI colours, so whatever palette is loaded drives it.
//! The active theme is kept in ~/.aiterm_theme, always rewritten in Ghostty's format.

pub type Rgb = (u8, u8, u8);

pub struct Theme {
    pub name: String,
    pub bg: Rgb,
    pub fg: Rgb,
    pub cursor: Option<Rgb>,
    pub palette: [Rgb; 16],
}

/// https://terminalcolors.com/themes/deus/default/
const DEUS: &str = "# aiterm theme: Deus
background = #2c323b
foreground = #eaeaea
cursor-color = #eaeaea
palette = 0=#242a32
palette = 1=#d54e53
palette = 2=#98c379
palette = 3=#e5c07b
palette = 4=#83a598
palette = 5=#c678dd
palette = 6=#70c0ba
palette = 7=#eaeaea
palette = 8=#666666
palette = 9=#ec3e45
palette = 10=#90c966
palette = 11=#edbf69
palette = 12=#73ba9f
palette = 13=#c858e9
palette = 14=#2bcec2
palette = 15=#ffffff
";

const NAMES: [&str; 8] = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];

pub fn deus() -> Theme {
    parse(DEUS, "Deus").expect("built-in theme")
}

fn path() -> Option<std::path::PathBuf> {
    crate::ai::home_dir().map(|h| h.join(".aiterm_theme"))
}

/// The saved theme, or Deus.
pub fn load() -> Theme {
    path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| parse(&s, "Custom").ok())
        .unwrap_or_else(deus)
}

pub fn save(t: &Theme) -> std::io::Result<()> {
    let p = path().ok_or_else(|| std::io::Error::other("no home directory"))?;
    std::fs::write(p, t.to_ghostty())
}

/// A terminalcolors.com theme page, a direct link to any supported file, or a local file.
pub fn fetch(source: &str) -> Result<Theme, String> {
    let source = source.trim().trim_matches(['"', '\'']);
    if source.starts_with("http://") || source.starts_with("https://") {
        let (url, name) = download_url(source);
        let text = ureq::get(&url)
            .timeout(std::time::Duration::from_secs(10))
            .call()
            .map_err(|e| format!("could not download the theme ({e})"))?
            .into_string()
            .map_err(|e| e.to_string())?;
        parse(&text, &name)
    } else {
        let p = std::path::Path::new(source);
        let text = std::fs::read_to_string(p).map_err(|e| format!("{source}: {e}"))?;
        let name = p.file_stem().map(|s| title(&s.to_string_lossy())).unwrap_or_else(|| "Custom".into());
        parse(&text, &name)
    }
}

/// `…/themes/deus/default/` → the Ghostty download for that theme; other links are used as given.
fn download_url(url: &str) -> (String, String) {
    let tail = url.split_once("terminalcolors.com/themes/").map(|(_, t)| t);
    if let Some(tail) = tail {
        let mut parts = tail.split(['/', '?', '#']).filter(|s| !s.is_empty());
        if let Some(theme) = parts.next() {
            let variant = parts.next().unwrap_or("default");
            let name = if variant == "default" { title(theme) } else { title(&format!("{theme} {variant}")) };
            return (format!("https://terminalcolors.com/downloads/ghostty/{theme}-{variant}"), name);
        }
    }
    let stem = url.trim_end_matches('/').rsplit('/').next().unwrap_or("custom");
    let stem = stem.split('.').next().unwrap_or(stem).trim_end_matches("-default");
    (url.to_string(), title(stem))
}

fn title(slug: &str) -> String {
    slug.split(['-', '_', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn parse(text: &str, fallback_name: &str) -> Result<Theme, String> {
    if text.contains("<plist") {
        return parse_iterm(text, fallback_name);
    }
    let mut bg = None;
    let mut fg = None;
    let mut cursor = None;
    let mut palette: [Option<Rgb>; 16] = [None; 16];
    let mut name = fallback_name.to_string();
    // Which half of the palette bare colour names refer to: Alacritty `[colors.bright]`, Warp `bright:`.
    let mut bright = false;
    let mut section = String::new();

    for raw in text.lines() {
        let line = raw.trim();
        if let Some(n) = line.strip_prefix("# aiterm theme:") {
            name = n.trim().to_string();
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(s) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section = s.to_string();
            bright = s.ends_with("bright");
            continue;
        }
        let Some((key, value)) = line.split_once(['=', ':']) else { continue };
        let key = key.trim().trim_matches('"');
        let value = value.trim().trim_matches('"');
        match key {
            "normal" | "bright" if value.is_empty() => bright = key == "bright",
            "name" if raw.starts_with("name") => name = value.to_string(),
            "background" if !section.contains("selection") => bg = hex(value).or(bg),
            "foreground" => fg = hex(value).or(fg),
            "cursor-color" | "cursor" => cursor = hex(value).or(cursor),
            "palette" => {
                if let Some((i, c)) = value.split_once('=') {
                    if let (Ok(i), Some(c)) = (i.trim().parse::<usize>(), hex(c)) {
                        if i < 16 {
                            palette[i] = Some(c);
                        }
                    }
                }
            }
            k => {
                if let Some(i) = NAMES.iter().position(|n| *n == k) {
                    if let Some(c) = hex(value) {
                        palette[i + if bright { 8 } else { 0 }] = Some(c);
                    }
                }
            }
        }
    }
    finish(name, bg, fg, cursor, palette)
}

/// iTerm2 `.itermcolors`: a plist of `<key>Ansi 3 Color</key><dict>…Red Component…</dict>`.
fn parse_iterm(text: &str, name: &str) -> Result<Theme, String> {
    let mut bg = None;
    let mut fg = None;
    let mut cursor = None;
    let mut palette: [Option<Rgb>; 16] = [None; 16];
    for chunk in text.split("<key>").skip(1) {
        let Some((key, body)) = chunk.split_once("</key>") else { continue };
        if !key.ends_with("Color") {
            continue;
        }
        let Some(dict) = body.split_once("</dict>").map(|(d, _)| d) else { continue };
        let component = |which: &str| -> Option<u8> {
            let after = dict.split_once(&format!("<key>{which} Component</key>"))?.1;
            let v = after.split_once("<real>")?.1.split_once("</real>")?.0;
            Some((v.trim().parse::<f32>().ok()?.clamp(0.0, 1.0) * 255.0).round() as u8)
        };
        let (Some(r), Some(g), Some(b)) = (component("Red"), component("Green"), component("Blue")) else { continue };
        let c = Some((r, g, b));
        match key {
            "Background Color" => bg = c,
            "Foreground Color" => fg = c,
            "Cursor Color" => cursor = c,
            k => {
                if let Some(i) = k.strip_prefix("Ansi ").and_then(|s| s.strip_suffix(" Color")).and_then(|s| s.parse::<usize>().ok()) {
                    if i < 16 {
                        palette[i] = c;
                    }
                }
            }
        }
    }
    finish(name.to_string(), bg, fg, cursor, palette)
}

fn finish(name: String, bg: Option<Rgb>, fg: Option<Rgb>, cursor: Option<Rgb>, p: [Option<Rgb>; 16]) -> Result<Theme, String> {
    let (Some(bg), Some(fg)) = (bg, fg) else { return Err("that file has no background/foreground colours".into()) };
    if p[..8].iter().any(Option::is_none) {
        return Err("that file has no ANSI palette".into());
    }
    let mut palette = [(0, 0, 0); 16];
    for i in 0..16 {
        // Themes without bright colours reuse the normal ones.
        palette[i] = p[i].or(p[i % 8]).unwrap();
    }
    Ok(Theme { name, bg, fg, cursor, palette })
}

fn hex(s: &str) -> Option<Rgb> {
    let s = s.trim().trim_matches(['"', '\'']);
    let h = s.strip_prefix('#').or_else(|| s.strip_prefix("0x"))?;
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

impl Theme {
    pub fn to_ghostty(&self) -> String {
        let c = |(r, g, b): Rgb| format!("#{r:02x}{g:02x}{b:02x}");
        let mut s = format!("# aiterm theme: {}\nbackground = {}\nforeground = {}\n", self.name, c(self.bg), c(self.fg));
        if let Some(cur) = self.cursor {
            s.push_str(&format!("cursor-color = {}\n", c(cur)));
        }
        for (i, p) in self.palette.iter().enumerate() {
            s.push_str(&format!("palette = {i}={}\n", c(*p)));
        }
        s
    }

    /// Recolour the window: the console's own colour table in a classic console window,
    /// OSC 4/10/11/12 in Windows Terminal and every other modern terminal.
    pub fn apply(&self) {
        #[cfg(windows)]
        if !modern_terminal() && console::apply(self) {
            return;
        }
        let mut s = String::new();
        let rgb = |(r, g, b): Rgb| format!("rgb:{r:02x}/{g:02x}/{b:02x}");
        for (i, c) in self.palette.iter().enumerate() {
            s.push_str(&format!("\x1b]4;{i};{}\x1b\\", rgb(*c)));
        }
        s.push_str(&format!("\x1b]10;{}\x1b\\\x1b]11;{}\x1b\\", rgb(self.fg), rgb(self.bg)));
        if let Some(c) = self.cursor {
            s.push_str(&format!("\x1b]12;{}\x1b\\", rgb(c)));
        }
        print!("{s}");
    }
}

/// Put the terminal's own colours back when aiterm shares its window with something else.
pub fn restore() {
    #[cfg(windows)]
    if !modern_terminal() {
        console::restore();
        return;
    }
    print!("\x1b]104\x1b\\\x1b]110\x1b\\\x1b]111\x1b\\\x1b]112\x1b\\");
}

#[cfg(windows)]
fn modern_terminal() -> bool {
    std::env::var_os("WT_SESSION").is_some() || std::env::var_os("TERM_PROGRAM").is_some()
}

#[cfg(windows)]
mod console {
    use super::{Rgb, Theme};
    use std::sync::Mutex;
    use windows_sys::Win32::System::Console::*;

    static ORIGINAL: Mutex<Option<CONSOLE_SCREEN_BUFFER_INFOEX>> = Mutex::new(None);

    /// The colour table is in Windows order (blue before red); ANSI index for each slot.
    const ANSI_FOR_SLOT: [usize; 8] = [0, 4, 2, 6, 1, 5, 3, 7];

    fn colorref((r, g, b): Rgb) -> u32 {
        r as u32 | (g as u32) << 8 | (b as u32) << 16
    }

    fn read() -> Option<(isize, CONSOLE_SCREEN_BUFFER_INFOEX)> {
        unsafe {
            let h = GetStdHandle(STD_OUTPUT_HANDLE);
            let mut info: CONSOLE_SCREEN_BUFFER_INFOEX = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<CONSOLE_SCREEN_BUFFER_INFOEX>() as u32;
            (GetConsoleScreenBufferInfoEx(h, &mut info) != 0).then_some((h as isize, info))
        }
    }

    fn write(h: isize, mut info: CONSOLE_SCREEN_BUFFER_INFOEX) -> bool {
        // The getter reports an inclusive window rect, the setter wants it exclusive; without this
        // the window shrinks by a row and a column on every call.
        info.srWindow.Right += 1;
        info.srWindow.Bottom += 1;
        unsafe { SetConsoleScreenBufferInfoEx(h as _, &info) != 0 }
    }

    pub fn apply(t: &Theme) -> bool {
        let Some((h, mut info)) = read() else { return false };
        ORIGINAL.lock().unwrap().get_or_insert(info);
        for (slot, ansi) in ANSI_FOR_SLOT.iter().enumerate() {
            info.ColorTable[slot] = colorref(t.palette[*ansi]);
            info.ColorTable[slot + 8] = colorref(t.palette[ansi + 8]);
        }
        // Default text is slot 7 on slot 0, so those two carry the theme's foreground and background.
        info.ColorTable[0] = colorref(t.bg);
        info.ColorTable[7] = colorref(t.fg);
        info.wAttributes = 0x07;
        info.wPopupAttributes = 0x70;
        write(h, info)
    }

    pub fn restore() {
        if let Some(orig) = ORIGINAL.lock().unwrap().take() {
            if let Some((h, now)) = read() {
                let mut back = orig;
                back.srWindow = now.srWindow;
                back.dwSize = now.dwSize;
                back.dwCursorPosition = now.dwCursorPosition;
                write(h, back);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_terminalcolors_formats() {
        let alacritty = "[colors.primary]\nforeground = \"#eaeaea\"\nbackground = \"#2c323b\"\n[colors.selection]\nbackground = \"#eaeaea\"\n[colors.normal]\nblack = \"#242a32\"\nred = \"#d54e53\"\ngreen = \"#98c379\"\nyellow = \"#e5c07b\"\nblue = \"#83a598\"\nmagenta = \"#c678dd\"\ncyan = \"#70c0ba\"\nwhite = \"#eaeaea\"\n[colors.bright]\nblack = \"#666666\"\nred = \"#ec3e45\"\n";
        let t = parse(alacritty, "x").unwrap();
        assert_eq!(t.bg, (0x2c, 0x32, 0x3b));
        assert_eq!(t.palette[8], (0x66, 0x66, 0x66));
        assert_eq!(t.palette[10], (0x98, 0xc3, 0x79)); // missing bright falls back to normal

        let warp = "name: Deus Default\naccent: \"#98c379\"\nbackground: \"#2c323b\"\nforeground: \"#eaeaea\"\nterminal_colors:\n  bright:\n    black: \"#666666\"\n  normal:\n    black: \"#242a32\"\n    blue: \"#83a598\"\n    cyan: \"#70c0ba\"\n    green: \"#98c379\"\n    magenta: \"#c678dd\"\n    red: \"#d54e53\"\n    white: \"#eaeaea\"\n    yellow: \"#e5c07b\"\n";
        let t = parse(warp, "x").unwrap();
        assert_eq!(t.name, "Deus Default");
        assert_eq!(t.palette[0], (0x24, 0x2a, 0x32));
        assert_eq!(t.palette[8], (0x66, 0x66, 0x66));

        let d = deus();
        assert_eq!(parse(&d.to_ghostty(), "x").unwrap().palette, d.palette);
        assert_eq!(d.name, "Deus");
    }

    #[test]
    fn theme_page_links_become_downloads() {
        let (url, name) = download_url("https://terminalcolors.com/themes/deus/default/");
        assert_eq!(url, "https://terminalcolors.com/downloads/ghostty/deus-default");
        assert_eq!(name, "Deus");
    }
}
