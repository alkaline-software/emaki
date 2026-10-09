//! A terminal of Emaki's own, with no window.
//!
//! Some things Claude Code takes only at its own prompt: ⇧Tab for the
//! mode, the `/model` and `/effort` pickers, a slash command, a dialog's
//! keys. For a session in the person's terminal the window reaches that
//! terminal. For a session with none, it used to open one. Now the
//! interactive `claude` runs here instead, on a pty this process owns,
//! and a screen model with no renderer keeps what it draws: `text` and
//! `styled` are that screen as the readers in `driver` take it
//! (`mode_on_screen`, `working_on_screen`, `dialog_on_screen`,
//! `suggestion_on_screen`), `rows` is the same screen as coloured
//! stretches for the window to draw when the person has to see it, and
//! `write` is the keyboard.
//!
//! The child is an ordinary interactive session: it registers in Claude
//! Code's registry with an inbox, runs the status line, and writes the
//! same transcript `claude --resume` in any terminal would. One writer
//! per transcript still holds, so whoever starts one must know no other
//! process is behind the session, and must let this one go before the
//! session is taken up in a terminal of the person's.
//!
//! Every live one is found by its child's pid (`for_pid`), which is what
//! the registry names a session by, so the window's terminal functions
//! can ask here first and fall through to the person's terminal app.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};

use crate::driver;

/// The screen's size. Wide enough that Claude Code's pickers are not
/// cut, narrow enough to draw in the window's column.
pub const ROWS: u16 = 40;
pub const COLS: u16 = 96;
/// How many rows that have left the top of the screen are kept, for the
/// terminal panel to scroll back through.
pub const SCROLLBACK: usize = 5000;

/// A stretch of one row drawn the same way.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    /// `0xRRGGBB`; None is the terminal's own ink or ground.
    pub fg: Option<u32>,
    pub bg: Option<u32>,
    /// Which of the sixteen named colours the ink or the ground is, when
    /// the program named one: a terminal draws those in its own palette.
    pub fg_ix: Option<u8>,
    pub bg_ix: Option<u8>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
}

impl Span {
    fn same_look(&self, o: &Span) -> bool {
        self.fg == o.fg && self.bg == o.bg && self.fg_ix == o.fg_ix && self.bg_ix == o.bg_ix && self.bold == o.bold && self.dim == o.dim && self.italic == o.italic && self.underline == o.underline && self.inverse == o.inverse
    }
}

pub struct Pty {
    /// The child's pid: Claude Code's registry names the session by it.
    pub pid: i32,
    parser: Mutex<vt100::Parser>,
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    alive: AtomicBool,
    started: Instant,
    last_output: Mutex<Instant>,
    /// What the program last asked to have put on the clipboard (OSC
    /// 52), until the window takes it.
    copied: Mutex<Option<String>>,
}

/// What a program hears of the mouse: a button let go, the pointer
/// moving with a button held, the pointer moving at all, and the form
/// the reports take.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MouseWant {
    pub release: bool,
    pub drag: bool,
    pub motion: bool,
    pub form: MouseForm,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MouseForm {
    /// `ESC [ M` and three bytes: nothing past column or row 223.
    Bytes,
    /// The same, each number a UTF-8 character.
    Utf8,
    /// `ESC [ < code ; column ; row` and `M`, or `m` for a button let go.
    Sgr,
}

/// One report of the mouse, as the bytes a terminal sends. `code` is the
/// button (0 left, 1 middle, 2 right; 64 and 65 the wheel up and down),
/// with 32 added while the pointer moves and 4, 8, 16 for Shift, Alt and
/// Control. The cell is counted from 0. Empty when the form cannot say it.
pub fn mouse_bytes(code: u8, col: u16, row: u16, press: bool, form: MouseForm) -> Vec<u8> {
    match form {
        MouseForm::Sgr => format!("\x1b[<{code};{};{}{}", col + 1, row + 1, if press { 'M' } else { 'm' }).into_bytes(),
        MouseForm::Bytes | MouseForm::Utf8 => {
            // A button let go is button 3 in these forms, the modifiers kept.
            let code = if press { code } else { code & !3 | 3 };
            let (b, x, y) = (32 + code as u32, 33 + col as u32, 33 + row as u32);
            let mut out = b"\x1b[M".to_vec();
            for v in [b, x, y] {
                if form == MouseForm::Utf8 {
                    let Some(c) = char::from_u32(v) else { return Vec::new() };
                    out.extend(c.to_string().as_bytes());
                } else if v <= 255 {
                    out.push(v as u8);
                } else {
                    return Vec::new();
                }
            }
            out
        }
    }
}

/// Every live one, for `for_pid`.
static LIVE: Mutex<Vec<Arc<Pty>>> = Mutex::new(Vec::new());

/// The hidden terminal whose child is `pid`, when there is one.
pub fn for_pid(pid: i32) -> Option<Arc<Pty>> {
    LIVE.lock().unwrap().iter().find(|p| p.pid == pid && p.alive()).cloned()
}

/// The command for an interactive Claude Code on `session_id`: resumed,
/// or begun under that id in the mode and with the model given ("" for
/// Claude Code's own defaults).
pub fn claude_argv(session_id: &str, resume: bool, mode: &str, model: &str) -> Vec<String> {
    let bin = driver::claude_binary().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| "claude".into());
    let mut argv = vec![bin, if resume { "--resume".into() } else { "--session-id".to_string() }, session_id.to_string()];
    if !resume {
        if !mode.is_empty() {
            argv.extend(["--permission-mode".to_string(), mode.to_string()]);
        }
        if !model.is_empty() && model != "default" {
            argv.extend(["--model".to_string(), model.to_string()]);
        }
    }
    argv
}

/// The person's own shell, as a login shell, for the terminal panel: what
/// `SHELL` names, else `sh`; on Windows what `COMSPEC` names, else `cmd`.
pub fn shell_argv() -> Vec<String> {
    if cfg!(windows) {
        return vec![std::env::var("COMSPEC").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "cmd.exe".into())];
    }
    vec![std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/sh".into()), "-l".into()]
}

/// The environment for the child: `driver::child_env`, as a terminal
/// that is nobody's in particular. The app may have been started from a
/// terminal app, and Claude Code reads that app's marks to decide which
/// keyboard protocol to speak; here it gets the plain one.
fn child_env() -> Vec<(String, String)> {
    const THEIRS: &[&str] = &["TERM", "COLORTERM", "TERM_PROGRAM", "TERM_PROGRAM_VERSION", "TERM_SESSION_ID", "LC_TERMINAL", "LC_TERMINAL_VERSION", "TMUX", "TMUX_PANE", "STY", "NO_COLOR", "FORCE_COLOR", "COLUMNS", "LINES", "__CFBundleIdentifier"];
    const THEIR_PREFIXES: &[&str] = &["WEZTERM_", "KITTY_", "ITERM_", "VSCODE_", "KAKU_", "GHOSTTY_", "ALACRITTY_", "KONSOLE_", "WT_"];
    let mut env: Vec<(String, String)> =
        driver::child_env().into_iter().filter(|(k, _)| !THEIRS.contains(&k.as_str()) && !THEIR_PREFIXES.iter().any(|p| k.starts_with(p))).collect();
    env.push(("TERM".into(), "xterm-256color".into()));
    env.push(("COLORTERM".into(), "truecolor".into()));
    env
}

/// The newest whole "set the clipboard" sequence in `pending` (OSC 52:
/// `ESC ] 52 ; <which> ; <base64>` ended by BEL or `ESC \`), decoded, and
/// `pending` cut down to what may still be the start of one. A program
/// run in a terminal copies this way when it draws its own selection,
/// as Claude Code does, and a terminal that ignores it leaves "copied"
/// on the screen and nothing on the clipboard. A question (`?`) asks
/// what the clipboard holds, and is not answered.
pub fn take_osc52(pending: &mut Vec<u8>) -> Option<String> {
    const START: &[u8] = b"\x1b]52;";
    const MOST: usize = 4 << 20;
    let mut found = None;
    loop {
        let Some(at) = pending.windows(START.len()).position(|w| w == START) else {
            // Nothing begun, but the last bytes may be the start of one.
            let keep = pending.len().saturating_sub(START.len() - 1);
            pending.drain(..keep);
            return found;
        };
        pending.drain(..at);
        let body = &pending[START.len()..];
        let Some(end) = body.iter().position(|b| *b == 0x07 || *b == 0x1b) else {
            if pending.len() > MOST {
                pending.clear();
            }
            return found;
        };
        let data = body[..end].splitn(2, |b| *b == b';').nth(1).unwrap_or(&[]);
        if let Some(text) = std::str::from_utf8(data).ok().filter(|d| *d != "?").and_then(driver::base64_decode).and_then(|b| String::from_utf8(b).ok()) {
            found = Some(text);
        }
        pending.drain(..START.len() + end + 1);
    }
}

/// The copy any live terminal of ours asked for, if one did.
pub fn take_copied() -> Option<String> {
    LIVE.lock().unwrap().iter().filter_map(|p| p.take_copied()).last()
}

impl Pty {
    /// Start `argv` in `cwd` on a pty of its own. `on_output` is called
    /// from the reading thread whenever the screen changed, and once more
    /// when the child is gone.
    pub fn spawn(argv: &[String], cwd: &str, on_output: Arc<dyn Fn() + Send + Sync>) -> Result<Arc<Pty>, String> {
        let (bin, rest) = argv.split_first().ok_or("nothing to run")?;
        let pair = native_pty_system().openpty(PtySize { rows: ROWS, cols: COLS, pixel_width: 0, pixel_height: 0 }).map_err(|e| e.to_string())?;
        let mut cmd = CommandBuilder::new(bin);
        cmd.args(rest);
        cmd.cwd(cwd);
        cmd.env_clear();
        for (k, v) in child_env() {
            cmd.env(k, v);
        }
        let mut child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave);
        let pid = child.process_id().ok_or("the child has no pid")? as i32;
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        let pty = Arc::new(Pty {
            pid,
            parser: Mutex::new(vt100::Parser::new(ROWS, COLS, SCROLLBACK)),
            writer: Mutex::new(writer),
            master: Mutex::new(pair.master),
            killer: Mutex::new(child.clone_killer()),
            alive: AtomicBool::new(true),
            started: Instant::now(),
            last_output: Mutex::new(Instant::now()),
            copied: Mutex::new(None),
        });
        LIVE.lock().unwrap().push(Arc::clone(&pty));
        let log = std::env::var_os("EMAKI_PTY_LOG").and_then(|p| std::fs::File::create(p).ok());
        let me = Arc::clone(&pty);
        std::thread::Builder::new()
            .name("emaki-pty".into())
            .spawn(move || {
                let mut log = log;
                let mut buf = [0u8; 8192];
                // What a query was cut off at, at the end of the last read.
                let mut tail: Vec<u8> = Vec::new();
                // A copy the program asks for, which may come in pieces.
                let mut osc: Vec<u8> = Vec::new();
                loop {
                    let n = match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    if let Some(f) = log.as_mut() {
                        let _ = f.write_all(&buf[..n]);
                    }
                    me.parser.lock().unwrap().process(&buf[..n]);
                    *me.last_output.lock().unwrap() = Instant::now();
                    tail.extend_from_slice(&buf[..n]);
                    me.answer_queries(&tail);
                    osc.extend_from_slice(&buf[..n]);
                    if let Some(text) = take_osc52(&mut osc) {
                        *me.copied.lock().unwrap() = Some(text);
                    }
                    let keep = tail.len().saturating_sub(8);
                    tail.drain(..keep);
                    // A sequence answered once is not kept to be answered again.
                    if tail.ends_with(b"n") || tail.ends_with(b"c") {
                        tail.clear();
                    }
                    on_output();
                }
                me.alive.store(false, Ordering::SeqCst);
                LIVE.lock().unwrap().retain(|p| !Arc::ptr_eq(p, &me));
                on_output();
            })
            .map_err(|e| e.to_string())?;
        // The child is waited for, so it leaves no zombie behind.
        let me = Arc::clone(&pty);
        std::thread::spawn(move || {
            let _ = child.wait();
            me.alive.store(false, Ordering::SeqCst);
        });
        Ok(pty)
    }

    /// What a program asks its terminal and waits on: where the cursor
    /// is (`ESC [ 6 n`) and what kind of terminal this is (`ESC [ c`).
    /// The screen model answers nothing by itself.
    fn answer_queries(&self, recent: &[u8]) {
        let has = |needle: &[u8]| recent.windows(needle.len()).any(|w| w == needle);
        if has(b"\x1b[6n") {
            let (row, col) = self.parser.lock().unwrap().screen().cursor_position();
            self.write(format!("\x1b[{};{}R", row + 1, col + 1).as_bytes());
        }
        if has(b"\x1b[c") || has(b"\x1b[0c") {
            self.write(b"\x1b[?62;c");
        }
    }

    /// The text the program asked to have copied since this was last
    /// asked, once. A terminal with no window has no clipboard of its
    /// own: the window puts it on the system's.
    pub fn take_copied(&self) -> Option<String> {
        self.copied.lock().unwrap().take()
    }

    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    /// How long ago the screen last changed.
    pub fn quiet_for(&self) -> std::time::Duration {
        self.last_output.lock().unwrap().elapsed()
    }

    pub fn age(&self) -> std::time::Duration {
        self.started.elapsed()
    }

    /// Keys, as bytes.
    pub fn write(&self, bytes: &[u8]) {
        let mut w = self.writer.lock().unwrap();
        let _ = w.write_all(bytes);
        let _ = w.flush();
    }

    /// Words put in the prompt the way a paste arrives, so new lines in
    /// them stay new lines and send nothing.
    pub fn paste(&self, text: &str) {
        let bracketed = self.parser.lock().unwrap().screen().bracketed_paste();
        if bracketed {
            self.write(format!("\x1b[200~{text}\x1b[201~").as_bytes());
        } else {
            self.write(text.replace('\n', " ").as_bytes());
        }
    }

    pub fn resize(&self, rows: u16, cols: u16) {
        let _ = self.master.lock().unwrap().resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        self.parser.lock().unwrap().screen_mut().set_size(rows, cols);
    }

    /// The screen's size: rows, columns.
    pub fn size(&self) -> (u16, u16) {
        self.parser.lock().unwrap().screen().size()
    }

    /// Where the cursor is, row and column, unless the program hid it.
    pub fn cursor(&self) -> Option<(u16, u16)> {
        let parser = self.parser.lock().unwrap();
        let screen = parser.screen();
        (!screen.hide_cursor()).then(|| screen.cursor_position())
    }

    /// Whether the program asked for the arrow keys in their application
    /// form (`ESC O A`), as a full-screen editor does.
    pub fn app_cursor(&self) -> bool {
        self.parser.lock().unwrap().screen().application_cursor()
    }

    /// Whether the program has the screen to itself (an editor, a
    /// pager), where there is nothing to scroll back to.
    pub fn alt_screen(&self) -> bool {
        self.parser.lock().unwrap().screen().alternate_screen()
    }

    /// The screen as it was `back` rows ago, and how far back that
    /// really is (there may be fewer rows kept). The look is taken and
    /// the screen put back at once: the readers of `rows`, `text` and
    /// `styled` always see it as it is now.
    pub fn rows_back(&self, back: usize) -> (Vec<Vec<Span>>, usize) {
        let mut parser = self.parser.lock().unwrap();
        parser.screen_mut().set_scrollback(back);
        let at = parser.screen().scrollback();
        let rows = rows_of(parser.screen());
        parser.screen_mut().set_scrollback(0);
        (rows, at)
    }

    /// Which rows of that look run on into the next, a long line and
    /// not two: copied, such rows are one line.
    pub fn wraps_back(&self, back: usize) -> Vec<bool> {
        let mut parser = self.parser.lock().unwrap();
        parser.screen_mut().set_scrollback(back);
        let rows = parser.screen().size().0;
        let wraps = (0..rows).map(|r| parser.screen().row_wrapped(r)).collect();
        parser.screen_mut().set_scrollback(0);
        wraps
    }

    /// The screen and everything that has left it are forgotten, as a
    /// terminal's "clear" does, and what the program asked for (the
    /// mouse, pastes in brackets, the cursor) is kept. Not where a
    /// program has the screen to itself: `false` then, and nothing done.
    /// The program is not told; a redraw is the caller's to ask for.
    pub fn clear_all(&self) -> bool {
        use vt100::{MouseProtocolEncoding as E, MouseProtocolMode as M};
        let mut parser = self.parser.lock().unwrap();
        let screen = parser.screen();
        if screen.alternate_screen() {
            return false;
        }
        let mut again: Vec<u8> = Vec::new();
        for (on, seq) in [
            (screen.bracketed_paste(), &b"\x1b[?2004h"[..]),
            (screen.application_cursor(), b"\x1b[?1h"),
            (screen.application_keypad(), b"\x1b="),
            (screen.hide_cursor(), b"\x1b[?25l"),
            (screen.mouse_protocol_mode() == M::Press, b"\x1b[?9h"),
            (screen.mouse_protocol_mode() == M::PressRelease, b"\x1b[?1000h"),
            (screen.mouse_protocol_mode() == M::ButtonMotion, b"\x1b[?1002h"),
            (screen.mouse_protocol_mode() == M::AnyMotion, b"\x1b[?1003h"),
            (screen.mouse_protocol_encoding() == E::Utf8, b"\x1b[?1005h"),
            (screen.mouse_protocol_encoding() == E::Sgr, b"\x1b[?1006h"),
        ] {
            if on {
                again.extend(seq);
            }
        }
        parser.process(b"\x1bc");
        parser.process(&again);
        true
    }

    /// What the program asked to be told of the mouse, when anything.
    pub fn mouse(&self) -> Option<MouseWant> {
        use vt100::{MouseProtocolEncoding as E, MouseProtocolMode as M};
        let parser = self.parser.lock().unwrap();
        let screen = parser.screen();
        let mode = screen.mouse_protocol_mode();
        if mode == M::None {
            return None;
        }
        Some(MouseWant {
            release: mode != M::Press,
            drag: matches!(mode, M::ButtonMotion | M::AnyMotion),
            motion: mode == M::AnyMotion,
            form: match screen.mouse_protocol_encoding() {
                E::Sgr => MouseForm::Sgr,
                E::Utf8 => MouseForm::Utf8,
                E::Default => MouseForm::Bytes,
            },
        })
    }

    /// Let the child go, as closing a terminal's window does.
    pub fn kill(&self) {
        let _ = self.killer.lock().unwrap().kill();
        self.alive.store(false, Ordering::SeqCst);
        LIVE.lock().unwrap().retain(|p| p.pid != self.pid);
    }

    /// The screen as stretches of text, a row at a time.
    pub fn rows(&self) -> Vec<Vec<Span>> {
        rows_of(self.parser.lock().unwrap().screen())
    }

    /// The screen as plain text, a line a row.
    pub fn text(&self) -> String {
        plain(&self.rows())
    }

    /// The screen with its colours written out as a terminal writes them,
    /// which is what `driver`'s readers take.
    pub fn styled(&self) -> String {
        styled(&self.rows())
    }
}

/// xterm's 256 colours.
fn indexed(i: u8) -> u32 {
    const BASE: [u32; 16] = [
        0x000000, 0xcd3131, 0x0dbc79, 0xe5e510, 0x2472c8, 0xbc3fbc, 0x11a8cd, 0xe5e5e5, 0x666666, 0xf14c4c, 0x23d18b, 0xf5f543, 0x3b8eea, 0xd670d6, 0x29b8db, 0xffffff,
    ];
    match i {
        0..=15 => BASE[i as usize],
        16..=231 => {
            let i = i as u32 - 16;
            let level = |v: u32| if v == 0 { 0 } else { 55 + 40 * v };
            level(i / 36) << 16 | level(i / 6 % 6) << 8 | level(i % 6)
        }
        _ => {
            let v = 8 + 10 * (i as u32 - 232);
            v << 16 | v << 8 | v
        }
    }
}

fn rgb(c: vt100::Color) -> Option<u32> {
    match c {
        vt100::Color::Default => None,
        vt100::Color::Idx(i) => Some(indexed(i)),
        vt100::Color::Rgb(r, g, b) => Some((r as u32) << 16 | (g as u32) << 8 | b as u32),
    }
}

fn named(c: vt100::Color) -> Option<u8> {
    match c {
        vt100::Color::Idx(i) if i < 16 => Some(i),
        _ => None,
    }
}

/// A screen as stretches of text, a row at a time, blank to the right
/// of the last thing written.
pub fn rows_of(screen: &vt100::Screen) -> Vec<Vec<Span>> {
    let (rows, cols) = screen.size();
    (0..rows)
        .map(|r| {
            let mut out: Vec<Span> = Vec::new();
            for c in 0..cols {
                let Some(cell) = screen.cell(r, c) else { continue };
                if cell.is_wide_continuation() {
                    continue;
                }
                let span = Span {
                    text: if cell.has_contents() { cell.contents().to_string() } else { " ".into() },
                    fg: rgb(cell.fgcolor()),
                    bg: rgb(cell.bgcolor()),
                    fg_ix: named(cell.fgcolor()),
                    bg_ix: named(cell.bgcolor()),
                    bold: cell.bold(),
                    dim: cell.dim(),
                    italic: cell.italic(),
                    underline: cell.underline(),
                    inverse: cell.inverse(),
                };
                match out.last_mut() {
                    Some(last) if last.same_look(&span) => last.text.push_str(&span.text),
                    _ => out.push(span),
                }
            }
            // Nothing drawn after the last mark: unstyled blanks go.
            while out.last().is_some_and(|s| s.bg.is_none() && !s.inverse && !s.underline && s.text.trim_end_matches(' ').is_empty()) {
                out.pop();
            }
            if let Some(last) = out.last_mut().filter(|s| s.bg.is_none() && !s.inverse && !s.underline) {
                last.text.truncate(last.text.trim_end_matches(' ').len());
            }
            out
        })
        .collect()
}

/// What a screen of that size holds once `bytes` have been written to
/// it: the screen model without a pty, for a test or a recorded log.
pub fn rows_after(bytes: &[u8], rows: u16, cols: u16) -> Vec<Vec<Span>> {
    let mut parser = vt100::Parser::new(rows, cols, 0);
    parser.process(bytes);
    rows_of(parser.screen())
}

/// The part of the screen worth drawing when the person has to see it.
/// Claude Code opens a picker (`/model`, `/effort`) under a line of
/// "▔" at the foot of the screen, with the conversation above it: only
/// the picker is kept. Any other screen is kept whole, without its blank
/// edges and with each run of blank rows as one.
/// Whether a picker is on the screen: the line of "▔" Claude Code opens
/// one under.
pub fn picker_up(rows: &[Vec<Span>]) -> bool {
    rows.iter().any(|row| row.iter().flat_map(|s| s.text.chars()).filter(|c| *c == '▔').count() >= 20)
}

pub fn panel_rows(rows: Vec<Vec<Span>>) -> Vec<Vec<Span>> {
    let text = |row: &Vec<Span>| row.iter().map(|s| s.text.as_str()).collect::<String>();
    let blank = |row: &Vec<Span>| text(row).trim().is_empty();
    let edge = rows.iter().rposition(|row| text(row).chars().filter(|c| *c == '▔').count() >= 20);
    let mut out: Vec<Vec<Span>> = Vec::new();
    for row in rows.into_iter().skip(edge.map(|e| e + 1).unwrap_or(0)) {
        if blank(&row) && out.last().is_none_or(blank) {
            continue;
        }
        out.push(row);
    }
    while out.last().is_some_and(blank) {
        out.pop();
    }
    out
}

/// A stretch of a row the pointer can press, and the keys that press it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub row: usize,
    /// Where it starts and ends in the row, in characters.
    pub start: usize,
    pub end: usize,
    pub keys: Vec<u8>,
}

/// What on a screen takes a click. Claude Code's own interface takes no
/// mouse, so a click is turned into the keys that do the same, for the
/// three things its pickers are made of: a numbered choice ("❯ 2. Opus
/// 5.5"), reached from the one the pointer is on with that many arrows up
/// or down; a level under a slider ("low medium high"), reached from the
/// one under the "▲" with arrows left or right; and a key the screen names
/// ("Enter to confirm", "s for this session only", "Esc to cancel", "Tab
/// to toggle"), which is that key. A click on a choice or a level also
/// confirms it, with Return after the arrows, so the one the pointer is
/// already on is Return alone; `s`, for the session only, stays a key
/// named at the foot.
pub fn hits(rows: &[Vec<Span>]) -> Vec<Hit> {
    let lines: Vec<Vec<char>> = rows.iter().map(|row| row.iter().flat_map(|s| s.text.chars()).collect()).collect();
    // A numbered choice: whether the pointer is on it, its number, and
    // where its text starts.
    let choice = |line: &[char]| -> Option<(bool, i64, usize)> {
        let start = line.iter().position(|c| !c.is_whitespace())?;
        let mut at = start;
        let pointed = line[at] == '❯';
        if matches!(line[at], '❯' | '↓' | '↑') {
            at += 1;
            while line.get(at).is_some_and(|c| c.is_whitespace()) {
                at += 1;
            }
        }
        let digits: String = line[at..].iter().take_while(|c| c.is_ascii_digit()).collect();
        let after = at + digits.len();
        (!digits.is_empty() && line.get(after) == Some(&'.') && line.get(after + 1).is_some_and(|c| c.is_whitespace())).then(|| (pointed, digits.parse().unwrap_or(0), start))
    };
    // To the choice, and Return on it: a click picks, as it does anywhere.
    let arrows = |steps: i64, back: &str, on: &str| -> Vec<u8> { format!("{}\r", (if steps < 0 { back } else { on }).repeat(steps.unsigned_abs() as usize)).into_bytes() };
    let pointed = lines.iter().find_map(|l| choice(l).filter(|c| c.0).map(|c| c.1));
    let mut out = Vec::new();
    for (row, line) in lines.iter().enumerate() {
        if let Some((_, n, start)) = choice(line) {
            if let Some(from) = pointed {
                let mut keys = arrows(n - from, "\x1b[A", "\x1b[B");
                // "Type something" is a field: the pointer goes there and
                // the person types. Return on it empty ended the question
                // unanswered.
                if line.iter().collect::<String>().contains("Type something") {
                    keys.pop();
                }
                out.push(Hit { row, start, end: line.len(), keys });
            }
            continue;
        }
        // The levels under a slider: the row above is the rule with its mark.
        let slider = row.checked_sub(1).map(|r| &lines[r]).and_then(|above| {
            let mark = above.iter().position(|c| *c == '▲')?;
            let from = above.iter().position(|c| matches!(c, '─' | '▲'))?;
            let to = above.iter().rposition(|c| matches!(c, '─' | '▲'))?;
            Some((mark, from, to))
        });
        // The row's pieces: what stands between runs of spaces or "·".
        let mut pieces: Vec<(usize, usize)> = Vec::new();
        let mut begin = None;
        for at in 0..=line.len() {
            let gap = at == line.len() || line[at] == '·' || (line[at] == ' ' && (line.get(at + 1).is_none_or(|c| matches!(c, ' ' | '·')) || at.checked_sub(1).is_some_and(|b| matches!(line[b], ' ' | '·'))));
            match (gap, begin) {
                (false, None) => begin = Some(at),
                (true, Some(b)) => {
                    pieces.push((b, at));
                    begin = None;
                }
                _ => {}
            }
        }
        if let Some((mark, from, to)) = slider {
            // On a slider's row a level is one word.
            let mut words: Vec<(usize, usize)> = Vec::new();
            let mut begin = None;
            for at in 0..=line.len() {
                let gap = at == line.len() || line[at].is_whitespace();
                match (gap, begin) {
                    (false, None) => begin = Some(at),
                    (true, Some(b)) => {
                        words.push((b, at));
                        begin = None;
                    }
                    _ => {}
                }
            }
            let levels: Vec<(usize, usize)> = words.into_iter().filter(|(a, b)| *a + 1 >= from && *b <= to + 2).collect();
            let centre = |(a, b): &(usize, usize)| (a + b) as i64;
            if let Some(on) = levels.iter().enumerate().min_by_key(|(_, l)| (centre(l) - 2 * mark as i64 - 1).abs()).map(|(ix, _)| ix) {
                for (ix, (a, b)) in levels.iter().enumerate() {
                    out.push(Hit { row, start: *a, end: *b, keys: arrows(ix as i64 - on as i64, "\x1b[D", "\x1b[C") });
                }
                pieces.retain(|(a, _)| *a > to + 2);
            }
        }
        // A key the screen names: "<key> to …" or "<key> for …".
        for (a, b) in pieces {
            let text: String = line[a..b].iter().collect();
            let mut words = text.split_whitespace();
            let key: &[u8] = match words.next() {
                Some("Enter") => b"\r",
                Some("Esc") => b"\x1b",
                Some("Tab") => b"\t",
                Some("Space") => b" ",
                Some(k) if k.len() == 1 && k.as_bytes()[0].is_ascii_alphanumeric() => k.as_bytes(),
                _ => continue,
            };
            if matches!(words.next(), Some("to" | "for")) && words.next().is_some() {
                out.push(Hit { row, start: a, end: b, keys: key.to_vec() });
            }
        }
    }
    out
}

pub fn plain(rows: &[Vec<Span>]) -> String {
    rows.iter().map(|row| row.iter().map(|s| s.text.as_str()).collect::<String>().trim_end_matches(' ').to_string()).collect::<Vec<_>>().join("\n")
}

/// The rows with each stretch's look written before it, one sequence an
/// attribute, which is the form every reader in `driver` takes.
pub fn styled(rows: &[Vec<Span>]) -> String {
    let mut out = String::new();
    for (ix, row) in rows.iter().enumerate() {
        if ix > 0 {
            out.push('\n');
        }
        let mut styled = false;
        for s in row {
            let plain = s.fg.is_none() && s.bg.is_none() && !(s.bold || s.dim || s.italic || s.underline || s.inverse);
            if styled || !plain {
                out.push_str("\x1b[0m");
            }
            styled = !plain;
            for (on, code) in [(s.bold, 1), (s.dim, 2), (s.italic, 3), (s.underline, 4), (s.inverse, 7)] {
                if on {
                    out.push_str(&format!("\x1b[{code}m"));
                }
            }
            for (color, code) in [(s.bg, 48), (s.fg, 38)] {
                if let Some(c) = color {
                    out.push_str(&format!("\x1b[{code};2;{};{};{}m", c >> 16 & 0xff, c >> 8 & 0xff, c & 0xff));
                }
            }
            out.push_str(&s.text);
        }
        if styled {
            out.push_str("\x1b[0m");
        }
    }
    out
}
