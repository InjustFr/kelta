//! Headless terminal model: one `alacritty_terminal::Term` per session (ARCHITECTURE D2, D7, §7.3-§7.4).
//!
//! The parser drives the `Term` through [`Shadowed`], a transparent `Handler` wrapper that also
//! records the few pieces of state `Term` keeps private but a snapshot must reproduce (active
//! charset, scroll region, title and title stack, urxvt mouse encoding, last printed character).

use std::sync::Arc;
use std::time::Instant;

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Cursor, Dimensions, Grid, Row};
use alacritty_terminal::index::Line;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::{ClipboardType, Config, Osc52, Term, TermMode};
use alacritty_terminal::vte::ansi::{
    self, Attr, CharsetIndex, ClearMode, CursorShape, CursorStyle, Handler, Hyperlink, KeyboardModes,
    KeyboardModesApplyBehavior, LineClearMode, Mode, ModifyOtherKeys, NamedPrivateMode, PrivateMode, Processor, Rgb,
    ScpCharPath, ScpUpdateMode, StandardCharset, TabulationClearMode,
};
use kelta_proto::term::{ClipboardKind, TerminalEvent};
use parking_lot::Mutex;

use crate::palette::Palette;
use crate::prescan::{OscScanner, Tail};

/// Maximum depth of the title stack (alacritty's own limit).
const TITLE_STACK_MAX_DEPTH: usize = 4096;
/// urxvt mouse encoding (`?1015`), which alacritty does not track.
const URXVT_MOUSE: u16 = 1015;

/// Collects `Term` events; drained after every `advance`.
#[derive(Clone, Default)]
pub struct Listener {
    events: Arc<Mutex<Vec<Event>>>,
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        match event {
            // High-frequency renderer hints a headless model does not need.
            Event::Wakeup | Event::MouseCursorDirty | Event::CursorBlinkingChange => {}
            other => self.events.lock().push(other),
        }
    }
}

/// Dimensions for `Term::new` / `Term::resize`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub cols: usize,
    pub rows: usize,
}

impl Size {
    pub fn new(cols: u16, rows: u16) -> Self {
        Self { cols: usize::from(cols.max(2)), rows: usize::from(rows.max(1)) }
    }
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// State `Term` keeps private, mirrored by [`Shadowed`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shadow {
    pub active_charset: CharsetIndex,
    /// Scroll region as `start..end` (0-based, end exclusive).
    pub scroll_top: usize,
    pub scroll_bottom: usize,
    pub title: Option<String>,
    pub title_stack: Vec<Option<String>>,
    pub urxvt_mouse: bool,
    pub last_char: Option<char>,
}

impl Shadow {
    fn new(rows: usize) -> Self {
        Self {
            active_charset: CharsetIndex::G0,
            scroll_top: 0,
            scroll_bottom: rows,
            title: None,
            title_stack: Vec::new(),
            urxvt_mouse: false,
            last_char: None,
        }
    }

    /// Scroll region covers the whole screen.
    pub fn full_region(&self, rows: usize) -> bool {
        self.scroll_top == 0 && self.scroll_bottom == rows
    }
}

/// Transparent `Handler` wrapper: every call is forwarded to `Term`; a few are mirrored.
pub struct Shadowed<'a> {
    pub term: &'a mut Term<Listener>,
    pub shadow: &'a mut Shadow,
}

impl Shadowed<'_> {
    fn reset_region(&mut self) {
        self.shadow.scroll_top = 0;
        self.shadow.scroll_bottom = self.term.screen_lines();
    }
}

impl Handler for Shadowed<'_> {
    #[inline]
    fn set_title(&mut self, title: Option<String>) {
        self.shadow.title.clone_from(&title);
        self.term.set_title(title);
    }
    #[inline]
    fn set_cursor_style(&mut self, s: Option<CursorStyle>) {
        self.term.set_cursor_style(s)
    }
    #[inline]
    fn set_cursor_shape(&mut self, shape: CursorShape) {
        self.term.set_cursor_shape(shape)
    }
    #[inline(always)]
    fn input(&mut self, c: char) {
        self.shadow.last_char = Some(c);
        self.term.input(c)
    }
    #[inline]
    fn goto(&mut self, line: i32, col: usize) {
        self.term.goto(line, col)
    }
    #[inline]
    fn goto_line(&mut self, line: i32) {
        self.term.goto_line(line)
    }
    #[inline]
    fn goto_col(&mut self, col: usize) {
        self.term.goto_col(col)
    }
    #[inline]
    fn insert_blank(&mut self, n: usize) {
        self.term.insert_blank(n)
    }
    #[inline]
    fn move_up(&mut self, n: usize) {
        self.term.move_up(n)
    }
    #[inline]
    fn move_down(&mut self, n: usize) {
        self.term.move_down(n)
    }
    #[inline]
    fn identify_terminal(&mut self, intermediate: Option<char>) {
        self.term.identify_terminal(intermediate)
    }
    #[inline]
    fn device_status(&mut self, n: usize) {
        self.term.device_status(n)
    }
    #[inline]
    fn move_forward(&mut self, col: usize) {
        self.term.move_forward(col)
    }
    #[inline]
    fn move_backward(&mut self, col: usize) {
        self.term.move_backward(col)
    }
    #[inline]
    fn move_down_and_cr(&mut self, row: usize) {
        self.term.move_down_and_cr(row)
    }
    #[inline]
    fn move_up_and_cr(&mut self, row: usize) {
        self.term.move_up_and_cr(row)
    }
    #[inline]
    fn put_tab(&mut self, count: u16) {
        self.term.put_tab(count)
    }
    #[inline]
    fn backspace(&mut self) {
        self.term.backspace()
    }
    #[inline]
    fn carriage_return(&mut self) {
        self.term.carriage_return()
    }
    #[inline]
    fn linefeed(&mut self) {
        self.term.linefeed()
    }
    #[inline]
    fn bell(&mut self) {
        self.term.bell()
    }
    #[inline]
    fn substitute(&mut self) {
        self.term.substitute()
    }
    #[inline]
    fn newline(&mut self) {
        self.term.newline()
    }
    #[inline]
    fn set_horizontal_tabstop(&mut self) {
        self.term.set_horizontal_tabstop()
    }
    #[inline]
    fn scroll_up(&mut self, n: usize) {
        self.term.scroll_up(n)
    }
    #[inline]
    fn scroll_down(&mut self, n: usize) {
        self.term.scroll_down(n)
    }
    #[inline]
    fn insert_blank_lines(&mut self, n: usize) {
        self.term.insert_blank_lines(n)
    }
    #[inline]
    fn delete_lines(&mut self, n: usize) {
        self.term.delete_lines(n)
    }
    #[inline]
    fn erase_chars(&mut self, n: usize) {
        self.term.erase_chars(n)
    }
    #[inline]
    fn delete_chars(&mut self, n: usize) {
        self.term.delete_chars(n)
    }
    #[inline]
    fn move_backward_tabs(&mut self, count: u16) {
        self.term.move_backward_tabs(count)
    }
    #[inline]
    fn move_forward_tabs(&mut self, count: u16) {
        self.term.move_forward_tabs(count)
    }
    #[inline]
    fn save_cursor_position(&mut self) {
        self.term.save_cursor_position()
    }
    #[inline]
    fn restore_cursor_position(&mut self) {
        self.term.restore_cursor_position()
    }
    #[inline]
    fn clear_line(&mut self, mode: LineClearMode) {
        self.term.clear_line(mode)
    }
    #[inline]
    fn clear_screen(&mut self, mode: ClearMode) {
        self.term.clear_screen(mode)
    }
    #[inline]
    fn clear_tabs(&mut self, mode: TabulationClearMode) {
        self.term.clear_tabs(mode)
    }
    #[inline]
    fn set_tabs(&mut self, interval: u16) {
        self.term.set_tabs(interval)
    }
    fn reset_state(&mut self) {
        self.term.reset_state();
        let rows = self.term.screen_lines();
        *self.shadow = Shadow::new(rows);
    }
    #[inline]
    fn reverse_index(&mut self) {
        self.term.reverse_index()
    }
    #[inline]
    fn terminal_attribute(&mut self, attr: Attr) {
        self.term.terminal_attribute(attr)
    }
    #[inline]
    fn set_mode(&mut self, mode: Mode) {
        self.term.set_mode(mode)
    }
    #[inline]
    fn unset_mode(&mut self, mode: Mode) {
        self.term.unset_mode(mode)
    }
    #[inline]
    fn report_mode(&mut self, mode: Mode) {
        self.term.report_mode(mode)
    }
    fn set_private_mode(&mut self, mode: PrivateMode) {
        match mode {
            PrivateMode::Unknown(URXVT_MOUSE) => self.shadow.urxvt_mouse = true,
            PrivateMode::Named(NamedPrivateMode::ColumnMode) => self.reset_region(),
            _ => {}
        }
        self.term.set_private_mode(mode)
    }
    fn unset_private_mode(&mut self, mode: PrivateMode) {
        match mode {
            PrivateMode::Unknown(URXVT_MOUSE) => self.shadow.urxvt_mouse = false,
            PrivateMode::Named(NamedPrivateMode::ColumnMode) => self.reset_region(),
            _ => {}
        }
        self.term.unset_private_mode(mode)
    }
    #[inline]
    fn report_private_mode(&mut self, mode: PrivateMode) {
        self.term.report_private_mode(mode)
    }
    fn set_scrolling_region(&mut self, top: usize, bottom: Option<usize>) {
        // Mirror of `Term::set_scrolling_region`.
        let rows = self.term.screen_lines();
        let b = bottom.unwrap_or(rows);
        if top < b {
            self.shadow.scroll_top = (top.saturating_sub(1)).min(rows);
            self.shadow.scroll_bottom = b.min(rows);
        }
        self.term.set_scrolling_region(top, bottom)
    }
    #[inline]
    fn set_keypad_application_mode(&mut self) {
        self.term.set_keypad_application_mode()
    }
    #[inline]
    fn unset_keypad_application_mode(&mut self) {
        self.term.unset_keypad_application_mode()
    }
    #[inline]
    fn set_active_charset(&mut self, index: CharsetIndex) {
        self.shadow.active_charset = index;
        self.term.set_active_charset(index)
    }
    #[inline]
    fn configure_charset(&mut self, index: CharsetIndex, charset: StandardCharset) {
        self.term.configure_charset(index, charset)
    }
    #[inline]
    fn set_color(&mut self, index: usize, color: Rgb) {
        self.term.set_color(index, color)
    }
    #[inline]
    fn dynamic_color_sequence(&mut self, prefix: String, index: usize, terminator: &str) {
        self.term.dynamic_color_sequence(prefix, index, terminator)
    }
    #[inline]
    fn reset_color(&mut self, index: usize) {
        self.term.reset_color(index)
    }
    #[inline]
    fn clipboard_store(&mut self, clipboard: u8, base64: &[u8]) {
        self.term.clipboard_store(clipboard, base64)
    }
    #[inline]
    fn clipboard_load(&mut self, clipboard: u8, terminator: &str) {
        self.term.clipboard_load(clipboard, terminator)
    }
    #[inline]
    fn decaln(&mut self) {
        self.term.decaln()
    }
    fn push_title(&mut self) {
        if self.shadow.title_stack.len() >= TITLE_STACK_MAX_DEPTH {
            self.shadow.title_stack.remove(0);
        }
        self.shadow.title_stack.push(self.shadow.title.clone());
        self.term.push_title()
    }
    fn pop_title(&mut self) {
        if let Some(t) = self.shadow.title_stack.pop() {
            self.shadow.title = t;
        }
        self.term.pop_title()
    }
    #[inline]
    fn text_area_size_pixels(&mut self) {
        self.term.text_area_size_pixels()
    }
    #[inline]
    fn text_area_size_chars(&mut self) {
        self.term.text_area_size_chars()
    }
    #[inline]
    fn set_hyperlink(&mut self, link: Option<Hyperlink>) {
        self.term.set_hyperlink(link)
    }
    #[inline]
    fn set_mouse_cursor_icon(&mut self, icon: ansi::cursor_icon::CursorIcon) {
        self.term.set_mouse_cursor_icon(icon)
    }
    #[inline]
    fn report_keyboard_mode(&mut self) {
        self.term.report_keyboard_mode()
    }
    #[inline]
    fn push_keyboard_mode(&mut self, mode: KeyboardModes) {
        self.term.push_keyboard_mode(mode)
    }
    #[inline]
    fn pop_keyboard_modes(&mut self, to_pop: u16) {
        self.term.pop_keyboard_modes(to_pop)
    }
    #[inline]
    fn set_keyboard_mode(&mut self, mode: KeyboardModes, behavior: KeyboardModesApplyBehavior) {
        self.term.set_keyboard_mode(mode, behavior)
    }
    #[inline]
    fn set_modify_other_keys(&mut self, mode: ModifyOtherKeys) {
        self.term.set_modify_other_keys(mode)
    }
    #[inline]
    fn report_modify_other_keys(&mut self) {
        self.term.report_modify_other_keys()
    }
    #[inline]
    fn set_scp(&mut self, char_path: ScpCharPath, update_mode: ScpUpdateMode) {
        self.term.set_scp(char_path, update_mode)
    }
}

/// What the model produced while parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    /// Bytes to write back to the PTY (query replies).
    Reply(Vec<u8>),
    /// An event for core.
    Event(TerminalEvent),
}

/// One session's terminal model.
pub struct TermModel {
    term: Term<Listener>,
    processor: Processor,
    shadow: Shadow,
    events: Arc<Mutex<Vec<Event>>>,
    osc: OscScanner,
    tail: Tail,
    history_limit: usize,
}

/// Model configuration (D7: kitty keyboard off; OSC 52 copy only).
fn config(history: usize) -> Config {
    Config { scrolling_history: history, kitty_keyboard: false, osc52: Osc52::OnlyCopy, ..Config::default() }
}

impl TermModel {
    pub fn new(cols: u16, rows: u16, history: usize) -> Self {
        let size = Size::new(cols, rows);
        let listener = Listener::default();
        let events = listener.events.clone();
        Self {
            term: Term::new(config(history), &size, listener),
            processor: Processor::new(),
            shadow: Shadow::new(size.rows),
            events,
            osc: OscScanner::new(),
            tail: Tail::new(),
            history_limit: history,
        }
    }

    /// Parse PTY output; replies and events are appended to `out`.
    pub fn advance(&mut self, bytes: &[u8], palette: &Palette, out: &mut Vec<Output>) {
        let mut evs = Vec::new();
        self.osc.scan(bytes, &mut evs);
        self.tail.update(bytes);
        let mut h = Shadowed { term: &mut self.term, shadow: &mut self.shadow };
        self.processor.advance(&mut h, bytes);
        out.extend(evs.into_iter().map(Output::Event));
        self.drain(palette, out);
    }

    /// Parse without collecting replies/events (benches, tests).
    pub fn feed(&mut self, bytes: &[u8]) {
        let mut out = Vec::new();
        self.advance(bytes, &Palette::default(), &mut out);
    }

    /// DEC 2026 synchronized-update deadline, if an update is pending.
    pub fn sync_deadline(&self) -> Option<Instant> {
        self.processor.sync_timeout().sync_timeout()
    }

    /// Bytes buffered by a pending synchronized update.
    pub fn sync_pending(&self) -> usize {
        self.processor.sync_bytes_count()
    }

    /// End a synchronized update (deadline expired, snapshot, exit).
    pub fn flush_sync(&mut self, palette: &Palette, out: &mut Vec<Output>) {
        if self.processor.sync_timeout().sync_timeout().is_none() {
            return;
        }
        let mut h = Shadowed { term: &mut self.term, shadow: &mut self.shadow };
        self.processor.stop_sync(&mut h);
        self.drain(palette, out);
    }

    fn drain(&mut self, palette: &Palette, out: &mut Vec<Output>) {
        let events = std::mem::take(&mut *self.events.lock());
        for ev in events {
            match ev {
                Event::PtyWrite(s) => out.push(Output::Reply(s.into_bytes())),
                Event::ColorRequest(index, fmt) => {
                    let rgb = self.term.colors()[index].unwrap_or_else(|| palette.color(index));
                    out.push(Output::Reply(fmt(rgb).into_bytes()));
                }
                Event::Title(t) => out.push(Output::Event(TerminalEvent::Title(t))),
                Event::ResetTitle => out.push(Output::Event(TerminalEvent::Title(String::new()))),
                Event::Bell => out.push(Output::Event(TerminalEvent::Bell)),
                Event::ClipboardStore(ty, text) => {
                    out.push(Output::Event(TerminalEvent::ClipboardStore { kind: clipboard_kind(ty), text }))
                }
                Event::ClipboardLoad(ty, _) => {
                    out.push(Output::Event(TerminalEvent::ClipboardLoad { kind: clipboard_kind(ty) }))
                }
                // TextAreaSizeRequest (CSI 14 t) needs pixel sizes the headless model does not know.
                _ => {}
            }
        }
    }

    /// Resize the model (reflow). Returns false when the size is unchanged.
    pub fn resize(&mut self, cols: u16, rows: u16) -> bool {
        let size = Size::new(cols, rows);
        if size.cols == self.term.columns() && size.rows == self.term.screen_lines() {
            return false;
        }
        self.term.resize(size);
        self.shadow.scroll_top = 0;
        self.shadow.scroll_bottom = size.rows;
        true
    }

    pub fn cols(&self) -> u16 {
        u16::try_from(self.term.columns()).unwrap_or(u16::MAX)
    }

    pub fn rows(&self) -> u16 {
        u16::try_from(self.term.screen_lines()).unwrap_or(u16::MAX)
    }

    /// Scrollback lines of the primary screen.
    pub fn history_size(&mut self) -> usize {
        if self.term.mode().contains(TermMode::ALT_SCREEN) {
            with_primary(&mut self.term, |g| g.history_size())
        } else {
            self.term.grid().history_size()
        }
    }

    pub fn history_limit(&self) -> usize {
        self.history_limit
    }

    /// Change the scrollback limit (shrinks history immediately when lowered).
    pub fn set_history_limit(&mut self, lines: usize) {
        if lines == self.history_limit {
            return;
        }
        self.history_limit = lines;
        self.term.set_options(config(lines));
        // `set_options` re-announces the title; that is not a change.
        self.events.lock().clear();
        self.release_cache();
    }

    /// Free rows alacritty pre-allocated beyond the used history (it grows in steps of 1000 rows).
    pub fn release_cache(&mut self) {
        if self.term.mode().contains(TermMode::ALT_SCREEN) {
            // The primary grid is inactive; its cache is released when the app leaves the alt screen.
            return;
        }
        self.term.grid_mut().truncate();
    }

    /// Plain text of the last `max_lines` lines of the primary screen (wrapped lines joined,
    /// trailing blank lines dropped).
    pub fn text_tail(&mut self, max_lines: usize) -> String {
        with_primary(&mut self.term, |g| grid_text(g, max_lines))
    }

    /// ANSI repaint (ARCHITECTURE §9.3) of the current state with at most `max_history` lines
    /// of scrollback, appended to `out`. Flushes a pending synchronized update first.
    pub fn snapshot_into(&mut self, max_history: usize, palette: &Palette, out: &mut Vec<u8>) -> Vec<Output> {
        let mut replies = Vec::new();
        self.flush_sync(palette, &mut replies);
        crate::snapshot::encode(
            &mut self.term,
            &self.shadow,
            self.osc.cwd_uri(),
            self.tail.pending(),
            max_history,
            out,
        );
        replies
    }

    /// Snapshot payload (no frame tag).
    pub fn snapshot(&mut self, max_history: usize) -> Vec<u8> {
        let mut out = Vec::new();
        let _ = self.snapshot_into(max_history, &Palette::default(), &mut out);
        out
    }

    /// Estimated model memory: `history × cols × 24 B` + both screens.
    pub fn memory_bytes(&mut self) -> u64 {
        let cols = self.term.columns() as u64;
        let rows = self.term.screen_lines() as u64;
        (self.history_size() as u64 + 2 * rows) * cols * 24
    }

    pub fn term(&self) -> &Term<Listener> {
        &self.term
    }

    pub fn term_mut(&mut self) -> &mut Term<Listener> {
        &mut self.term
    }

    pub fn shadow(&self) -> &Shadow {
        &self.shadow
    }

    pub fn cwd_uri(&self) -> Option<&str> {
        self.osc.cwd_uri()
    }
}

fn clipboard_kind(ty: ClipboardType) -> ClipboardKind {
    match ty {
        ClipboardType::Clipboard => ClipboardKind::Clipboard,
        ClipboardType::Selection => ClipboardKind::Primary,
    }
}

/// Run `f` on the primary grid. `Term` only exposes the active grid, so while the alternate
/// screen is active the grids are swapped and the alternate screen is restored afterwards
/// (rows, cursor, saved cursor). Side effect: the primary saved cursor becomes the primary
/// cursor, which is what entering `?1049` already did.
pub fn with_primary<R>(term: &mut Term<Listener>, f: impl FnOnce(&Grid<Cell>) -> R) -> R {
    if !term.mode().contains(TermMode::ALT_SCREEN) {
        return f(term.grid());
    }
    let rows = term.screen_lines();
    let alt_rows: Vec<Row<Cell>> = (0..rows).map(|i| term.grid()[Line(i as i32)].clone()).collect();
    let alt_cursor: Cursor<Cell> = term.grid().cursor.clone();
    let alt_saved: Cursor<Cell> = term.grid().saved_cursor.clone();
    term.swap_alt();
    let r = f(term.grid());
    term.swap_alt();
    let g = term.grid_mut();
    for (i, row) in alt_rows.into_iter().enumerate() {
        g[Line(i as i32)] = row;
    }
    g.cursor = alt_cursor;
    g.saved_cursor = alt_saved;
    r
}

/// Text of the last `max_lines` logical rows of a grid.
pub fn grid_text(g: &Grid<Cell>, max_lines: usize) -> String {
    let cols = g.columns();
    let top = -(g.history_size() as i32);
    let bottom = g.screen_lines() as i32;
    // Last non-empty row.
    let mut last = bottom - 1;
    while last >= top && row_is_blank(&g[Line(last)], cols) {
        last -= 1;
    }
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for l in top..=last {
        let row = &g[Line(l)];
        let mut s = String::new();
        for c in 0..cols {
            let cell = &row[alacritty_terminal::index::Column(c)];
            if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                continue;
            }
            s.push(if cell.c == '\t' { ' ' } else { cell.c });
            if let Some(zw) = cell.zerowidth() {
                s.extend(zw.iter());
            }
        }
        let wrapped = row[alacritty_terminal::index::Column(cols - 1)].flags.contains(Flags::WRAPLINE);
        if wrapped {
            cur.push_str(&s);
        } else {
            cur.push_str(s.trim_end());
            lines.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        lines.push(cur.trim_end().to_owned());
    }
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].join("\n")
}

fn row_is_blank(row: &Row<Cell>, cols: usize) -> bool {
    (0..cols).all(|c| {
        let cell = &row[alacritty_terminal::index::Column(c)];
        (cell.c == ' ' || cell.c == '\t') && cell.zerowidth().is_none()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn replies(m: &mut TermModel, bytes: &[u8]) -> Vec<String> {
        let mut out = Vec::new();
        m.advance(bytes, &Palette::default(), &mut out);
        out.into_iter()
            .filter_map(|o| match o {
                Output::Reply(r) => Some(String::from_utf8_lossy(&r).into_owned()),
                Output::Event(_) => None,
            })
            .collect()
    }

    #[test]
    fn answers_basic_queries_once() {
        let mut m = TermModel::new(80, 24, 100);
        assert_eq!(replies(&mut m, b"\x1b[c"), vec!["\x1b[?6c"]);
        assert_eq!(replies(&mut m, b"\x1b[5n"), vec!["\x1b[0n"]);
        assert_eq!(replies(&mut m, b"ab\x1b[6n"), vec!["\x1b[1;3R"]);
    }

    #[test]
    fn palette_replies_use_pushed_palette_and_overrides() {
        let mut m = TermModel::new(80, 24, 100);
        let mut pal = kelta_proto::term::TerminalPalette::default();
        pal.background = "#123456".into();
        pal.ansi[2] = "#00ff00".into();
        let pal = Palette::from_proto(&pal);
        let mut out = Vec::new();
        m.advance(b"\x1b]11;?\x07\x1b]4;2;?\x1b\\\x1b]10;?\x07", &pal, &mut out);
        let r: Vec<_> = out
            .iter()
            .filter_map(|o| match o {
                Output::Reply(r) => Some(String::from_utf8_lossy(r).into_owned()),
                Output::Event(_) => None,
            })
            .collect();
        assert_eq!(
            r,
            vec![
                "\x1b]11;rgb:1212/3434/5656\x07".to_owned(),
                "\x1b]4;2;rgb:0000/ffff/0000\x1b\\".to_owned(),
                "\x1b]10;rgb:d4d4/d4d4/d4d4\x07".to_owned(),
            ]
        );
        // An application override wins over the pushed palette.
        let mut out = Vec::new();
        m.advance(b"\x1b]11;#ff0000\x07\x1b]11;?\x07", &pal, &mut out);
        assert_eq!(out, vec![Output::Reply(b"\x1b]11;rgb:ffff/0000/0000\x07".to_vec())]);
    }

    #[test]
    fn kitty_keyboard_is_off() {
        let mut m = TermModel::new(80, 24, 100);
        assert!(replies(&mut m, b"\x1b[>1u\x1b[?u").is_empty());
        assert!(!m.term().mode().intersects(TermMode::KITTY_KEYBOARD_PROTOCOL));
    }

    #[test]
    fn events_title_bell_clipboard_notify() {
        let mut m = TermModel::new(80, 24, 100);
        let mut out = Vec::new();
        m.advance(b"\x1b]2;hello\x07\x07\x1b]52;c;aGk=\x07\x1b]777;notify;T;B\x07", &Palette::default(), &mut out);
        assert_eq!(
            out,
            vec![
                Output::Event(TerminalEvent::Notify { title: Some("T".into()), body: "B".into() }),
                Output::Event(TerminalEvent::Title("hello".into())),
                Output::Event(TerminalEvent::Bell),
                Output::Event(TerminalEvent::ClipboardStore { kind: ClipboardKind::Clipboard, text: "hi".into() }),
            ]
        );
        // OSC 52 read is not allowed (copy only).
        let mut out = Vec::new();
        m.advance(b"\x1b]52;c;?\x07", &Palette::default(), &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn shadow_tracks_private_state() {
        let mut m = TermModel::new(80, 24, 100);
        m.feed(b"\x1b[3;10r\x0e\x1b]2;one\x07\x1b[22t\x1b]2;two\x07\x1b[?1015h");
        let s = m.shadow().clone();
        assert_eq!((s.scroll_top, s.scroll_bottom), (2, 10));
        assert_eq!(s.active_charset, CharsetIndex::G1);
        assert_eq!(s.title.as_deref(), Some("two"));
        assert!(s.urxvt_mouse);
        m.feed(b"\x1b[23t\x0f");
        assert_eq!(m.shadow().title.as_deref(), Some("one"));
        assert_eq!(m.shadow().active_charset, CharsetIndex::G0);
        m.resize(100, 30);
        assert!(m.shadow().full_region(30));
        m.feed(b"\x1b[5;6r\x1bc");
        assert!(m.shadow().full_region(30));
        assert_eq!(m.shadow().title, None);
    }

    #[test]
    fn sync_update_is_buffered_until_flush() {
        let mut m = TermModel::new(20, 5, 10);
        m.feed(b"\x1b[?2026hHELLO");
        assert!(m.sync_deadline().is_some());
        assert!(!m.text_tail(5).contains("HELLO"));
        let mut out = Vec::new();
        m.flush_sync(&Palette::default(), &mut out);
        assert!(m.text_tail(5).contains("HELLO"));
        assert!(m.sync_deadline().is_none());
    }

    #[test]
    fn text_tail_joins_wrapped_lines() {
        let mut m = TermModel::new(10, 3, 100);
        m.feed(b"line1\r\n0123456789abc\r\nlast\r\n");
        assert_eq!(m.text_tail(10), "line1\n0123456789abc\nlast");
        assert_eq!(m.text_tail(1), "last");
    }

    #[test]
    fn with_primary_restores_alt_screen() {
        let mut m = TermModel::new(10, 3, 100);
        m.feed(b"main1\r\nmain2\x1b[?1049h\x1b[2;3Halt\x1b[1;1H\x1b7\x1b[3;2H");
        let before: Vec<Row<Cell>> = (0..3).map(|i| m.term().grid()[Line(i)].clone()).collect();
        let cursor = m.term().grid().cursor.clone();
        assert_eq!(m.text_tail(5), "main1\nmain2");
        let after: Vec<Row<Cell>> = (0..3).map(|i| m.term().grid()[Line(i)].clone()).collect();
        assert!(before == after);
        assert_eq!(m.term().grid().cursor, cursor);
        assert!(m.term().mode().contains(TermMode::ALT_SCREEN));
    }

    #[test]
    fn history_limit_shrinks() {
        let mut m = TermModel::new(10, 2, 1000);
        for i in 0..600 {
            m.feed(format!("{i}\r\n").as_bytes());
        }
        assert_eq!(m.history_size(), 599);
        m.set_history_limit(500);
        assert_eq!(m.history_size(), 500);
    }
}
