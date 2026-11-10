//! ~/.aiterm_theme, which aiterm always writes in Ghostty's format (see src/theme.rs), as the
//! JSON the page turns into an xterm.js theme. Missing or broken: aiterm's built-in Deus.

pub type Rgb = (u8, u8, u8);

pub struct Theme {
    pub bg: Rgb,
    pub fg: Rgb,
    pub cursor: Rgb,
    pub palette: [Rgb; 16],
}

/// Same colours as `DEUS` in src/theme.rs.
const DEUS: [u32; 18] = [
    0x2c323b, 0xeaeaea, // background, foreground (cursor = foreground)
    0x242a32, 0xd54e53, 0x98c379, 0xe5c07b, 0x83a598, 0xc678dd, 0x70c0ba, 0xeaeaea, //
    0x666666, 0xec3e45, 0x90c966, 0xedbf69, 0x73ba9f, 0xc858e9, 0x2bcec2, 0xffffff,
];

fn rgb(v: u32) -> Rgb {
    ((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

fn hex(s: &str) -> Option<Rgb> {
    let h = s.trim().trim_matches(['"', '\'']).strip_prefix('#')?;
    (h.len() == 6).then(|| u32::from_str_radix(h, 16).ok().map(rgb)).flatten()
}

fn deus() -> Theme {
    let mut palette = [(0, 0, 0); 16];
    for (i, p) in palette.iter_mut().enumerate() {
        *p = rgb(DEUS[i + 2]);
    }
    Theme { bg: rgb(DEUS[0]), fg: rgb(DEUS[1]), cursor: rgb(DEUS[1]), palette }
}

pub fn load() -> Theme {
    let home = std::env::var_os("USERPROFILE").map(std::path::PathBuf::from);
    home.and_then(|h| std::fs::read_to_string(h.join(".aiterm_theme")).ok()).and_then(|s| parse(&s)).unwrap_or_else(deus)
}

fn parse(text: &str) -> Option<Theme> {
    let (mut bg, mut fg, mut cursor) = (None, None, None);
    let mut p: [Option<Rgb>; 16] = [None; 16];
    for line in text.lines().map(str::trim).filter(|l| !l.starts_with('#')) {
        let Some((key, value)) = line.split_once('=') else { continue };
        match key.trim() {
            "background" => bg = hex(value),
            "foreground" => fg = hex(value),
            "cursor-color" => cursor = hex(value),
            "palette" => {
                if let Some((i, c)) = value.split_once('=') {
                    if let (Ok(i @ 0..=15), Some(c)) = (i.trim().parse::<usize>(), hex(c)) {
                        p[i] = Some(c);
                    }
                }
            }
            _ => {}
        }
    }
    let (bg, fg) = (bg?, fg?);
    let mut palette = [(0, 0, 0); 16];
    for i in 0..16 {
        palette[i] = p[i].or(p[i % 8])?; // no bright half: reuse the normal one, as aiterm does
    }
    Some(Theme { bg, fg, cursor: cursor.unwrap_or(fg), palette })
}

impl Theme {
    pub fn dark(&self) -> bool {
        let (r, g, b) = self.bg;
        (r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000 < 128
    }

    pub fn json(&self) -> String {
        let c = |(r, g, b): Rgb| format!("\"#{r:02x}{g:02x}{b:02x}\"");
        let palette: Vec<String> = self.palette.iter().map(|p| c(*p)).collect();
        format!(
            "{{\"bg\":{},\"fg\":{},\"cursor\":{},\"palette\":[{}]}}",
            c(self.bg),
            c(self.fg),
            c(self.cursor),
            palette.join(",")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_aiterm_writes() {
        let t = parse("# aiterm theme: X\nbackground = #010203\nforeground = #fefefe\npalette = 0=#000000\npalette = 1=#110000\npalette = 2=#001100\npalette = 3=#111100\npalette = 4=#000011\npalette = 5=#110011\npalette = 6=#001111\npalette = 7=#cccccc\npalette = 9=#ff0000\n").unwrap();
        assert_eq!(t.bg, (1, 2, 3));
        assert_eq!(t.cursor, t.fg);
        assert_eq!(t.palette[9], (255, 0, 0));
        assert_eq!(t.palette[10], (0, 0x11, 0));
        assert!(t.dark());
        assert!(parse("background = #000000").is_none());
        assert_eq!(deus().palette[14], (0x2b, 0xce, 0xc2));
    }
}
