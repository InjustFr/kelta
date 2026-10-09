//! Snapshot encoder (ARCHITECTURE §9.3): an ANSI byte stream that, written into a freshly reset
//! terminal, reproduces the model's state.
//!
//! Coverage: alternate screen with the main scrollback (`?1049`), the last N history lines, every
//! visible cell with SGR runs (bold/dim/italic/inverse/hidden/strike, colon underline styles
//! `4:2..4:5`, underline colour `58`, truecolor / 256 / 16 colours), OSC 8 hyperlinks (with their
//! ids), wide and combining characters, soft wraps, tab cells, DECSTBM + origin mode, saved cursor
//! (DECSC, incl. attributes and charsets), G0-G3 charsets (DEC line drawing) and SO/SI, cursor
//! position (incl. the pending-wrap state), shape (DECSCUSR) and visibility (`?25`), DECCKM,
//! DECKPAM, auto-wrap `?7`, insert mode, LNM, bracketed paste `?2004`, mouse `?1000/1002/1003` with
//! encodings `?1005/1006/1015`, focus `?1004`, alternate scroll `?1007`, urgency `?1042`, sync
//! `?2026` (always off), OSC 4/10/11/12 colour overrides, title (OSC 2), cwd (OSC 7), and the
//! unfinished escape sequence / UTF-8 character at the end of the stream.

use std::io::Write as _;

use alacritty_terminal::grid::{Cursor, Dimensions, Grid, Row};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{CharsetIndex, Color, CursorShape, NamedColor, StandardCharset};

use crate::model::{Listener, Shadow, with_primary};

const STYLE: Flags = Flags::BOLD
    .union(Flags::DIM)
    .union(Flags::ITALIC)
    .union(Flags::INVERSE)
    .union(Flags::HIDDEN)
    .union(Flags::STRIKEOUT)
    .union(Flags::ALL_UNDERLINES);

/// Shortest run of blank cells replaced by a cursor move.
const SKIP_RUN: usize = 6;

const CHARSETS: [CharsetIndex; 4] = [CharsetIndex::G0, CharsetIndex::G1, CharsetIndex::G2, CharsetIndex::G3];

/// Graphic rendition of a cell (what SGR + OSC 8 can set).
#[derive(Debug, Clone, PartialEq)]
struct Pen {
    fg: Color,
    bg: Color,
    flags: Flags,
    underline: Option<Color>,
    link: Option<(String, String)>,
}

impl Default for Pen {
    fn default() -> Self {
        Self {
            fg: Color::Named(NamedColor::Foreground),
            bg: Color::Named(NamedColor::Background),
            flags: Flags::empty(),
            underline: None,
            link: None,
        }
    }
}

impl Pen {
    fn of(cell: &Cell) -> Self {
        Self {
            fg: cell.fg,
            bg: cell.bg,
            flags: cell.flags & STYLE,
            underline: cell.underline_color(),
            link: cell.hyperlink().map(|h| (h.id().to_owned(), h.uri().to_owned())),
        }
    }

    fn same_sgr(&self, other: &Self) -> bool {
        self.fg == other.fg
            && self.bg == other.bg
            && self.flags == other.flags
            && self.underline == other.underline
    }
}

/// How a row relates to the previous one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cont {
    /// Starts a new line (CR LF or CUP).
    None,
    /// Soft-wrapped continuation.
    Wrap,
    /// Continuation whose first (wide) character re-creates the previous row's leading spacer.
    LeadingSpacer,
}

/// A cell that a fresh terminal already holds (nothing to write).
fn is_blank(cell: &Cell) -> bool {
    cell.c == ' '
        && cell.fg == Color::Named(NamedColor::Foreground)
        && cell.bg == Color::Named(NamedColor::Background)
        && !cell.flags.intersects(!Flags::WRAPLINE)
        && cell.zerowidth().is_none()
        && cell.underline_color().is_none()
        && cell.hyperlink().is_none()
}

struct Writer<'a> {
    out: &'a mut Vec<u8>,
    pen: Pen,
    charsets: [StandardCharset; 4],
    cols: usize,
}

impl Writer<'_> {
    fn s(&mut self, s: &str) {
        self.out.extend_from_slice(s.as_bytes());
    }

    fn num(&mut self, n: usize) {
        let _ = write!(self.out, "{n}");
    }

    fn color(&mut self, base: usize, c: Color) {
        match c {
            Color::Named(n) => {
                let i = n as usize;
                match i {
                    0..=7 => {
                        self.s(";");
                        self.num(base + i);
                    }
                    8..=15 => {
                        self.s(";");
                        self.num(base + 60 + i - 8);
                    }
                    // Default colours (39/49) are the reset state.
                    _ => {}
                }
            }
            Color::Indexed(i) => {
                self.s(";");
                self.num(base + 8);
                self.s(";5;");
                self.num(usize::from(i));
            }
            Color::Spec(rgb) => {
                self.s(";");
                self.num(base + 8);
                let _ = write!(self.out, ";2;{};{};{}", rgb.r, rgb.g, rgb.b);
            }
        }
    }

    fn set_pen(&mut self, pen: &Pen) {
        if !self.pen.same_sgr(pen) {
            let from_default = self.pen.same_sgr(&Pen::default());
            let start = self.out.len();
            self.s("\x1b[0");
            let f = pen.flags;
            if f.contains(Flags::BOLD) {
                self.s(";1");
            }
            if f.contains(Flags::DIM) {
                self.s(";2");
            }
            if f.contains(Flags::ITALIC) {
                self.s(";3");
            }
            if f.contains(Flags::UNDERLINE) {
                self.s(";4");
            } else if f.contains(Flags::DOUBLE_UNDERLINE) {
                self.s(";4:2");
            } else if f.contains(Flags::UNDERCURL) {
                self.s(";4:3");
            } else if f.contains(Flags::DOTTED_UNDERLINE) {
                self.s(";4:4");
            } else if f.contains(Flags::DASHED_UNDERLINE) {
                self.s(";4:5");
            }
            if f.contains(Flags::INVERSE) {
                self.s(";7");
            }
            if f.contains(Flags::HIDDEN) {
                self.s(";8");
            }
            if f.contains(Flags::STRIKEOUT) {
                self.s(";9");
            }
            self.color(30, pen.fg);
            self.color(40, pen.bg);
            match pen.underline {
                Some(Color::Spec(rgb)) => {
                    let _ = write!(self.out, ";58;2;{};{};{}", rgb.r, rgb.g, rgb.b);
                }
                Some(Color::Indexed(i)) => {
                    let _ = write!(self.out, ";58;5;{i}");
                }
                Some(Color::Named(n)) if (n as usize) < 16 => {
                    let _ = write!(self.out, ";58;5;{}", n as usize);
                }
                _ => {}
            }
            self.s("m");
            if from_default && self.out.len() > start + 4 {
                // From the default pen the attributes are additive: drop the leading `0;`.
                self.out.drain(start + 2..start + 4);
            }
        }
        if self.pen.link != pen.link {
            match &pen.link {
                Some((id, uri)) => {
                    self.s("\x1b]8;id=");
                    self.s(&sanitize(id));
                    self.s(";");
                    self.s(&sanitize(uri));
                    self.s("\x1b\\");
                }
                None => self.s("\x1b]8;;\x1b\\"),
            }
        }
        self.pen = pen.clone();
    }

    /// Back to the default pen (before line feeds, which fill new rows with the template).
    fn reset_pen(&mut self) {
        let d = Pen::default();
        if self.pen != d {
            self.set_pen(&d);
        }
    }

    fn glyph(&mut self, cell: &Cell, c: char) {
        let pen = Pen::of(cell);
        self.set_pen(&pen);
        let mut buf = [0u8; 4];
        self.out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        if let Some(zw) = cell.zerowidth() {
            for z in zw {
                self.out.extend_from_slice(z.encode_utf8(&mut buf).as_bytes());
            }
        }
    }

    /// Write cells `0..end` of a row starting at column 0 with the cursor at column 0 (or, for a
    /// continuation row, pending wrap at the end of the previous row).
    fn row(&mut self, row: &Row<Cell>, end: usize, wrapped: bool, next_starts_wide: bool, cont: Cont) {
        let last = self.cols - 1;
        let mut skip = 0;
        if cont == Cont::Wrap {
            // Wrap with the default pen so a scroll fills the new row with the default background,
            // then return to its first column.
            self.reset_pen();
            self.s(" \r");
        }
        let allow_skip = cont != Cont::LeadingSpacer;
        for c in 0..end {
            if c < skip {
                continue;
            }
            let cell = &row[Column(c)];
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                if c > 0 && row[Column(c - 1)].flags.contains(Flags::WIDE_CHAR) {
                    continue;
                }
                self.glyph(cell, ' ');
                continue;
            }
            if cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER) {
                if c == last && wrapped && next_starts_wide {
                    // Re-created by the wide character on the next row.
                    continue;
                }
                self.glyph(cell, ' ');
                continue;
            }
            if cell.c == '\t' && !(c == last && wrapped) {
                // Tab cell: a space turned into '\t' by HT, then back to the next column.
                self.glyph(cell, ' ');
                self.s("\x1b[");
                self.num(c + 1);
                self.s("G\t\x1b[");
                self.num(c + 2);
                self.s("G");
                continue;
            }
            if is_blank(cell) {
                // Skip runs of untouched cells inside the row with a cursor move.
                let run = (c..end).take_while(|&i| is_blank(&row[Column(i)])).count();
                if allow_skip && run >= SKIP_RUN && c + run < end {
                    skip = c + run;
                    self.s("\x1b[");
                    self.num(skip + 1);
                    self.s("G");
                    continue;
                }
            }
            let ch = if cell.c == '\t' { ' ' } else { cell.c };
            self.glyph(cell, ch);
        }
        if cont == Cont::LeadingSpacer && end < self.cols {
            // The wide character wrapped with its own pen: clear the rest with the default one.
            self.reset_pen();
            self.s("\x1b[K");
        }
    }

    /// How a row continues the previous one.
    fn cont(&self, prev: Option<&Row<Cell>>, row: &Row<Cell>) -> Cont {
        let Some(prev) = prev.filter(|p| self.wrapped(p)) else { return Cont::None };
        let last = &prev[Column(self.cols - 1)];
        if last.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
            && row[Column(0)].flags.contains(Flags::WIDE_CHAR)
        {
            Cont::LeadingSpacer
        } else {
            Cont::Wrap
        }
    }

    /// Number of cells to write for a row.
    fn extent(&self, row: &Row<Cell>, wrapped: bool) -> usize {
        if wrapped {
            return self.cols;
        }
        let mut end = self.cols;
        while end > 0 && is_blank(&row[Column(end - 1)]) {
            end -= 1;
        }
        end
    }

    fn wrapped(&self, row: &Row<Cell>) -> bool {
        row[Column(self.cols - 1)].flags.contains(Flags::WRAPLINE)
    }

    /// Primary screen: history (at most `max_history` lines) then the visible rows, so the
    /// receiving terminal scrolls the same lines into its scrollback.
    fn primary(&mut self, g: &Grid<Cell>, max_history: usize) {
        let hist = g.history_size().min(max_history) as i32;
        let rows = g.screen_lines() as i32;
        let mut prev: Option<&Row<Cell>> = None;
        for l in -hist..rows {
            let row = &g[Line(l)];
            let cont = self.cont(prev, row);
            if prev.is_some() && cont == Cont::None {
                self.reset_pen();
                self.s("\r\n");
            }
            let wrapped = self.wrapped(row);
            let end = self.extent(row, wrapped);
            let next_wide = l + 1 < rows && g[Line(l + 1)][Column(0)].flags.contains(Flags::WIDE_CHAR);
            self.row(row, end, wrapped, next_wide, cont);
            prev = Some(row);
        }
    }

    /// Alternate screen rows (no scrollback): each row is positioned explicitly.
    fn alternate(&mut self, g: &Grid<Cell>) {
        let rows = g.screen_lines() as i32;
        let mut prev: Option<&Row<Cell>> = None;
        for l in 0..rows {
            let row = &g[Line(l)];
            let cont = self.cont(prev, row);
            let wrapped = self.wrapped(row) && l + 1 < rows;
            let end = self.extent(row, wrapped);
            prev = Some(row);
            if cont == Cont::None {
                if end == 0 {
                    continue;
                }
                self.cup(l as usize, 0, 0);
            }
            let next_wide = l + 1 < rows && g[Line(l + 1)][Column(0)].flags.contains(Flags::WIDE_CHAR);
            self.row(row, end, wrapped, next_wide, cont);
        }
    }

    /// CUP to an absolute (0-based) position; `origin` is the scroll-region top when DECOM is on.
    fn cup(&mut self, line: usize, col: usize, origin: usize) {
        self.s("\x1b[");
        self.num(line.saturating_sub(origin) + 1);
        self.s(";");
        self.num(col + 1);
        self.s("H");
    }

    /// Move to `cursor` and reproduce its pending-wrap state, pen and charsets.
    fn cursor(&mut self, g: &Grid<Cell>, cursor: &Cursor<Cell>, origin: usize) {
        let Point { line, column } = cursor.point;
        let line_idx = line.0.max(0) as usize;
        let last = self.cols - 1;
        let mut placed = false;
        if cursor.input_needs_wrap && column.0 == last {
            // Re-write the last cell so the terminal is left in the "wrap on next char" state.
            let row = &g[line];
            let cell = &row[Column(last)];
            if !cell.flags.intersects(Flags::WRAPLINE | Flags::LEADING_WIDE_CHAR_SPACER) {
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) && last > 0 {
                    let wide = &row[Column(last - 1)];
                    if wide.flags.contains(Flags::WIDE_CHAR) {
                        self.cup(line_idx, last - 1, origin);
                        self.glyph(wide, wide.c);
                        placed = true;
                    }
                } else {
                    self.cup(line_idx, last, origin);
                    let ch = if cell.c == '\t' { ' ' } else { cell.c };
                    self.glyph(cell, ch);
                    placed = true;
                }
            }
        }
        if !placed {
            self.cup(line_idx, column.0, origin);
        }
        self.set_pen(&Pen::of(&cursor.template));
        self.set_charsets(cursor);
    }

    fn set_charsets(&mut self, cursor: &Cursor<Cell>) {
        for (i, idx) in CHARSETS.iter().enumerate() {
            let want = cursor.charsets[*idx];
            if self.charsets[i] != want {
                let inter = ["(", ")", "*", "+"][i];
                let fin = match want {
                    StandardCharset::Ascii => "B",
                    StandardCharset::SpecialCharacterAndLineDrawing => "0",
                };
                self.s("\x1b");
                self.s(inter);
                self.s(fin);
                self.charsets[i] = want;
            }
        }
    }

    fn ascii_charsets(&mut self) {
        let ascii = Cursor::<Cell>::default();
        self.set_charsets(&ascii);
    }

    /// DECSC state of a grid (skipped when it is the power-on default).
    fn saved_cursor(&mut self, g: &Grid<Cell>) {
        if g.saved_cursor == Cursor::default() {
            return;
        }
        let saved = g.saved_cursor.clone();
        self.cursor(g, &saved, 0);
        self.s("\x1b7");
        self.ascii_charsets();
    }
}

fn sanitize(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect()
}

/// Encode the snapshot of `term` into `out` (see the module docs for the coverage).
pub fn encode(
    term: &mut Term<Listener>,
    shadow: &Shadow,
    cwd_uri: Option<&str>,
    pending: &[u8],
    max_history: usize,
    out: &mut Vec<u8>,
) {
    let cols = term.columns();
    let rows = term.screen_lines();
    let alt = term.mode().contains(TermMode::ALT_SCREEN);
    out.reserve((rows + max_history.min(term.grid().history_size())) * (cols + 4) + 256);

    let mut w = Writer { out, pen: Pen::default(), charsets: [StandardCharset::Ascii; 4], cols };
    // Full reset: the receiving terminal may not be fresh.
    w.s("\x1bc");

    // Colour overrides set by the application (OSC 4 / 10 / 11 / 12).
    {
        let colors = term.colors();
        for i in 0..256 {
            if let Some(rgb) = colors[i] {
                let _ = write!(w.out, "\x1b]4;{i};rgb:{:02x}/{:02x}/{:02x}\x07", rgb.r, rgb.g, rgb.b);
            }
        }
        for (osc, idx) in [(10, 256), (11, 257), (12, 258)] {
            if let Some(rgb) = colors[idx] {
                let _ = write!(w.out, "\x1b]{osc};rgb:{:02x}/{:02x}/{:02x}\x07", rgb.r, rgb.g, rgb.b);
            }
        }
    }

    if alt {
        with_primary(term, |g| {
            w.primary(g, max_history);
            let c = g.cursor.clone();
            w.cursor(g, &c, 0);
        });
        // Enter the alternate screen; it inherits the primary cursor (pen and charsets).
        w.s("\x1b[?1049h");
        w.reset_pen();
        w.ascii_charsets();
        // The alternate screen was cleared with the inherited background: clear it again.
        w.s("\x1b[2J");
        w.alternate(term.grid());
    } else {
        w.primary(term.grid(), max_history);
    }
    w.saved_cursor(term.grid());

    let mode = *term.mode();
    // Scroll region (DECSTBM homes the cursor; the final CUP comes later).
    let origin = if shadow.full_region(rows) { 0 } else { shadow.scroll_top };
    if !shadow.full_region(rows) {
        let _ = write!(w.out, "\x1b[{};{}r", shadow.scroll_top + 1, shadow.scroll_bottom);
    }

    // Private modes (DECOM last: it homes the cursor).
    let mut set: Vec<u16> = Vec::new();
    let mut reset: Vec<u16> = Vec::new();
    let mut flag = |on: bool, default: bool, n: u16| {
        if on != default {
            if on { set.push(n) } else { reset.push(n) }
        }
    };
    flag(mode.contains(TermMode::APP_CURSOR), false, 1);
    flag(mode.contains(TermMode::LINE_WRAP), true, 7);
    flag(mode.contains(TermMode::SHOW_CURSOR), true, 25);
    flag(mode.contains(TermMode::MOUSE_REPORT_CLICK), false, 1000);
    flag(mode.contains(TermMode::MOUSE_DRAG), false, 1002);
    flag(mode.contains(TermMode::MOUSE_MOTION), false, 1003);
    flag(mode.contains(TermMode::FOCUS_IN_OUT), false, 1004);
    flag(mode.contains(TermMode::UTF8_MOUSE), false, 1005);
    flag(mode.contains(TermMode::SGR_MOUSE), false, 1006);
    flag(mode.contains(TermMode::ALTERNATE_SCROLL), true, 1007);
    flag(shadow.urxvt_mouse, false, 1015);
    flag(mode.contains(TermMode::URGENCY_HINTS), true, 1042);
    flag(mode.contains(TermMode::BRACKETED_PASTE), false, 2004);
    for n in set {
        let _ = write!(w.out, "\x1b[?{n}h");
    }
    for n in reset {
        let _ = write!(w.out, "\x1b[?{n}l");
    }
    if mode.contains(TermMode::APP_KEYPAD) {
        w.s("\x1b=");
    }
    if mode.contains(TermMode::ORIGIN) {
        w.s("\x1b[?6h");
    }

    // Cursor: position (+ pending wrap), pen, charsets, shift state.
    let origin = if mode.contains(TermMode::ORIGIN) { origin } else { 0 };
    let cursor = term.grid().cursor.clone();
    w.cursor(term.grid(), &cursor, origin);
    if shadow.active_charset == CharsetIndex::G1 {
        w.s("\x0e");
    }

    // ANSI modes after the last printed character (insert mode would shift cells).
    if mode.contains(TermMode::INSERT) {
        w.s("\x1b[4h");
    }
    if mode.contains(TermMode::LINE_FEED_NEW_LINE) {
        w.s("\x1b[20h");
    }

    // Cursor shape (DECSCUSR).
    let style = term.cursor_style();
    if style != Config::default().default_cursor_style {
        let base = match style.shape {
            CursorShape::Block => Some(1),
            CursorShape::Underline => Some(3),
            CursorShape::Beam => Some(5),
            CursorShape::HollowBlock | CursorShape::Hidden => None,
        };
        if let Some(b) = base {
            let n = if style.blinking { b } else { b + 1 };
            let _ = write!(w.out, "\x1b[{n} q");
        }
    }

    if let Some(title) = &shadow.title {
        w.s("\x1b]2;");
        w.s(&sanitize(title));
        w.s("\x07");
    }
    if let Some(uri) = cwd_uri {
        w.s("\x1b]7;");
        w.s(&sanitize(uri));
        w.s("\x07");
    }
    // Synchronized output is never left on by a snapshot.
    w.s("\x1b[?2026l");
    w.out.extend_from_slice(pending);
}
