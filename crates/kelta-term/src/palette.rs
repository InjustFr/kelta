//! Colours used to answer OSC 4/10/11/12 queries (ARCHITECTURE §7.4), pushed by the UI through
//! `terminal_set_palette`.

use alacritty_terminal::vte::ansi::Rgb;
use kelta_proto::term::TerminalPalette;

/// alacritty colour index of the default foreground (OSC 10).
pub const FOREGROUND: usize = 256;
/// Default background (OSC 11).
pub const BACKGROUND: usize = 257;
/// Cursor colour (OSC 12).
pub const CURSOR: usize = 258;

/// Parsed palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    pub ansi: [Rgb; 16],
}

impl Default for Palette {
    fn default() -> Self {
        Self::from_proto(&TerminalPalette::default())
    }
}

impl Palette {
    /// Parse `#rrggbb` colours; invalid entries keep the built-in default.
    pub fn from_proto(p: &TerminalPalette) -> Self {
        let fallback = TerminalPalette::default();
        let pick =
            |s: &str, d: &str| parse_hex(s).or_else(|| parse_hex(d)).unwrap_or(Rgb { r: 0, g: 0, b: 0 });
        let mut ansi = [Rgb { r: 0, g: 0, b: 0 }; 16];
        for (i, slot) in ansi.iter_mut().enumerate() {
            let s = p.ansi.get(i).map(String::as_str).unwrap_or("");
            *slot = pick(s, &fallback.ansi[i]);
        }
        Self {
            foreground: pick(&p.foreground, &fallback.foreground),
            background: pick(&p.background, &fallback.background),
            cursor: pick(&p.cursor, &fallback.cursor),
            ansi,
        }
    }

    /// Colour of an alacritty colour index (0..=255 indexed, 256 fg, 257 bg, 258 cursor).
    pub fn color(&self, index: usize) -> Rgb {
        match index {
            0..=15 => self.ansi[index],
            16..=231 => {
                let i = index - 16;
                let level = |v: usize| if v == 0 { 0 } else { (55 + v * 40) as u8 };
                Rgb { r: level(i / 36), g: level((i / 6) % 6), b: level(i % 6) }
            }
            232..=255 => {
                let v = (8 + (index - 232) * 10) as u8;
                Rgb { r: v, g: v, b: v }
            }
            BACKGROUND => self.background,
            CURSOR => self.cursor,
            _ => self.foreground,
        }
    }
}

/// `#rrggbb` (or `rrggbb`) → Rgb.
pub fn parse_hex(s: &str) -> Option<Rgb> {
    let h = s.trim().strip_prefix('#').unwrap_or(s.trim());
    if h.len() != 6 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(Rgb { r: (v >> 16) as u8, g: (v >> 8) as u8, b: v as u8 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pushed_palette() {
        let mut p = TerminalPalette::default();
        p.background = "#102030".into();
        p.ansi[1] = "#ff0000".into();
        p.cursor = "bogus".into();
        let pal = Palette::from_proto(&p);
        assert_eq!(pal.color(BACKGROUND), Rgb { r: 0x10, g: 0x20, b: 0x30 });
        assert_eq!(pal.color(1), Rgb { r: 255, g: 0, b: 0 });
        // Invalid → default (#d4d4d4).
        assert_eq!(pal.color(CURSOR), Rgb { r: 0xd4, g: 0xd4, b: 0xd4 });
        assert_eq!(pal.color(FOREGROUND), Rgb { r: 0xd4, g: 0xd4, b: 0xd4 });
    }

    #[test]
    fn xterm_cube_and_ramp() {
        let pal = Palette::default();
        assert_eq!(pal.color(16), Rgb { r: 0, g: 0, b: 0 });
        assert_eq!(pal.color(196), Rgb { r: 255, g: 0, b: 0 });
        assert_eq!(pal.color(231), Rgb { r: 255, g: 255, b: 255 });
        assert_eq!(pal.color(232), Rgb { r: 8, g: 8, b: 8 });
        assert_eq!(pal.color(255), Rgb { r: 238, g: 238, b: 238 });
    }

    #[test]
    fn short_palettes_fall_back() {
        let p = TerminalPalette { ansi: vec!["#010203".into()], ..TerminalPalette::default() };
        let pal = Palette::from_proto(&p);
        assert_eq!(pal.color(0), Rgb { r: 1, g: 2, b: 3 });
        assert_eq!(pal.color(15), Rgb { r: 255, g: 255, b: 255 });
    }
}
