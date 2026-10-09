//! Comparable dump of a model's observable state (snapshot round-trip tests, debugging).

use alacritty_terminal::grid::{Cursor, Dimensions, Grid};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::cell::Cell;
use alacritty_terminal::vte::ansi::{CharsetIndex, Color, StandardCharset};

use crate::model::{TermModel, with_primary};

/// One cell, compared by value (alacritty's `Cell` equality also compares storage details).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellDump {
    pub c: char,
    pub zerowidth: Vec<char>,
    pub fg: Color,
    pub bg: Color,
    pub flags: u16,
    pub underline: Option<Color>,
    pub link: Option<(String, String)>,
}

impl CellDump {
    fn of(cell: &Cell) -> Self {
        Self {
            c: cell.c,
            zerowidth: cell.zerowidth().map(<[char]>::to_vec).unwrap_or_default(),
            fg: cell.fg,
            bg: cell.bg,
            flags: cell.flags.bits(),
            underline: cell.underline_color(),
            link: cell.hyperlink().map(|h| (h.id().to_owned(), h.uri().to_owned())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorDump {
    pub line: i32,
    pub column: usize,
    pub template: CellDump,
    pub charsets: [StandardCharset; 4],
    pub input_needs_wrap: bool,
}

impl CursorDump {
    fn of(c: &Cursor<Cell>) -> Self {
        Self {
            line: c.point.line.0,
            column: c.point.column.0,
            template: CellDump::of(&c.template),
            charsets: [
                c.charsets[CharsetIndex::G0],
                c.charsets[CharsetIndex::G1],
                c.charsets[CharsetIndex::G2],
                c.charsets[CharsetIndex::G3],
            ],
            input_needs_wrap: c.input_needs_wrap,
        }
    }
}

/// Observable state of a model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dump {
    pub cols: usize,
    pub rows: usize,
    /// Primary screen: history (oldest first, at most `max_history`) then the visible rows.
    pub primary: Vec<Vec<CellDump>>,
    /// Alternate screen rows when it is active.
    pub alternate: Option<Vec<Vec<CellDump>>>,
    /// Active grid cursor and saved cursor.
    pub cursor: CursorDump,
    pub saved_cursor: CursorDump,
    /// Primary cursor while the alternate screen is active.
    pub primary_cursor: Option<CursorDump>,
    pub modes: u32,
    pub cursor_shape: String,
    pub cursor_blinking: bool,
    pub active_charset: CharsetIndex,
    pub scroll_region: (usize, usize),
    pub title: Option<String>,
    pub urxvt_mouse: bool,
    pub colors: Vec<Option<(u8, u8, u8)>>,
    pub cwd: Option<String>,
}

fn rows_of(g: &Grid<Cell>, from: i32, to: i32) -> Vec<Vec<CellDump>> {
    (from..to)
        .map(|l| {
            let row = &g[Line(l)];
            (0..g.columns()).map(|c| CellDump::of(&row[Column(c)])).collect()
        })
        .collect()
}

/// Dump `m`, keeping at most `max_history` scrollback lines.
pub fn dump(m: &mut TermModel, max_history: usize) -> Dump {
    let alt = m.term().mode().contains(TermMode::ALT_SCREEN);
    let term = m.term_mut();
    let rows = term.screen_lines();
    let (primary, primary_cursor) = with_primary(term, |g| {
        let hist = g.history_size().min(max_history) as i32;
        (rows_of(g, -hist, g.screen_lines() as i32), CursorDump::of(&g.cursor))
    });
    let g = term.grid();
    let alternate = alt.then(|| rows_of(g, 0, rows as i32));
    let style = term.cursor_style();
    let colors = (0..269).map(|i| term.colors()[i].map(|c| (c.r, c.g, c.b))).collect();
    let cursor = CursorDump::of(&g.cursor);
    let saved_cursor = CursorDump::of(&g.saved_cursor);
    let modes = term.mode().bits();
    let cols = term.columns();
    let shadow = m.shadow().clone();
    Dump {
        cols,
        rows,
        primary,
        alternate,
        cursor,
        saved_cursor,
        primary_cursor: alt.then_some(primary_cursor),
        modes,
        cursor_shape: format!("{:?}", style.shape),
        cursor_blinking: style.blinking,
        active_charset: shadow.active_charset,
        scroll_region: (shadow.scroll_top, shadow.scroll_bottom),
        title: shadow.title,
        urxvt_mouse: shadow.urxvt_mouse,
        colors,
        cwd: m.cwd_uri().map(str::to_owned),
    }
}

/// First difference between two dumps, as a readable message (None when equal).
pub fn diff(a: &Dump, b: &Dump) -> Option<String> {
    if a == b {
        return None;
    }
    let text = |row: &[CellDump]| row.iter().map(|c| c.c).collect::<String>();
    if a.primary.len() != b.primary.len() {
        return Some(format!("primary line count {} != {}", a.primary.len(), b.primary.len()));
    }
    for (i, (ra, rb)) in a.primary.iter().zip(&b.primary).enumerate() {
        if ra != rb {
            let col = ra.iter().zip(rb).position(|(x, y)| x != y).unwrap_or(0);
            return Some(format!(
                "primary line {i} col {col}:\n  A {:?}\n  B {:?}\n  A |{}|\n  B |{}|",
                ra.get(col),
                rb.get(col),
                text(ra),
                text(rb)
            ));
        }
    }
    if a.alternate != b.alternate {
        if let (Some(x), Some(y)) = (&a.alternate, &b.alternate) {
            for (i, (ra, rb)) in x.iter().zip(y).enumerate() {
                if ra != rb {
                    let col = ra.iter().zip(rb).position(|(p, q)| p != q).unwrap_or(0);
                    return Some(format!(
                        "alternate line {i} col {col}:\n  A {:?}\n  B {:?}\n  A |{}|\n  B |{}|",
                        ra.get(col),
                        rb.get(col),
                        text(ra),
                        text(rb)
                    ));
                }
            }
        }
        return Some("alternate screen presence differs".into());
    }
    let mut parts = Vec::new();
    if a.cursor != b.cursor {
        parts.push(format!("cursor {:?} != {:?}", a.cursor, b.cursor));
    }
    if a.saved_cursor != b.saved_cursor {
        parts.push(format!("saved cursor {:?} != {:?}", a.saved_cursor, b.saved_cursor));
    }
    if a.primary_cursor != b.primary_cursor {
        parts.push(format!("primary cursor {:?} != {:?}", a.primary_cursor, b.primary_cursor));
    }
    if a.modes != b.modes {
        parts.push(format!("modes {:?} != {:?}", TermMode::from_bits_retain(a.modes), TermMode::from_bits_retain(b.modes)));
    }
    if (&a.cursor_shape, a.cursor_blinking) != (&b.cursor_shape, b.cursor_blinking) {
        parts.push(format!("cursor style {} {} != {} {}", a.cursor_shape, a.cursor_blinking, b.cursor_shape, b.cursor_blinking));
    }
    if a.active_charset != b.active_charset {
        parts.push(format!("active charset {:?} != {:?}", a.active_charset, b.active_charset));
    }
    if a.scroll_region != b.scroll_region {
        parts.push(format!("scroll region {:?} != {:?}", a.scroll_region, b.scroll_region));
    }
    if a.title != b.title {
        parts.push(format!("title {:?} != {:?}", a.title, b.title));
    }
    if a.urxvt_mouse != b.urxvt_mouse {
        parts.push("urxvt mouse differs".into());
    }
    if a.colors != b.colors {
        parts.push("colour overrides differ".into());
    }
    if a.cwd != b.cwd {
        parts.push(format!("cwd {:?} != {:?}", a.cwd, b.cwd));
    }
    if (a.cols, a.rows) != (b.cols, b.rows) {
        parts.push(format!("size {}x{} != {}x{}", a.cols, a.rows, b.cols, b.rows));
    }
    Some(parts.join("\n"))
}
