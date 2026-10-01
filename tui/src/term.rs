//! Terminal plumbing with no dependencies: raw mode via `stty`, a byte
//! reader thread, a key parser that understands UTF-8, arrows and bracketed
//! paste, and an ANSI truecolor screen buffer.

use std::io::{Read, Write};
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use tick_core::Rgb;

// ---------------------------------------------------------------------------
// raw mode
// ---------------------------------------------------------------------------

fn stty(args: &[&str]) -> Option<String> {
    let out = Command::new("stty")
        .args(args)
        .stdin(std::process::Stdio::inherit())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// (rows, cols)
pub fn size() -> Option<(usize, usize)> {
    let out = stty(&["size"])?;
    let mut it = out.split_whitespace();
    Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
}

pub struct Guard {
    saved: String,
}

impl Guard {
    pub fn enter(title: &str) -> Option<Guard> {
        let saved = stty(&["-g"])?;
        stty(&["raw", "-echo"])?;
        // alt screen, hide cursor, bracketed paste, window title
        print!("\x1b[?1049h\x1b[?25l\x1b[?2004h\x1b]0;{title}\x07");
        let _ = std::io::stdout().flush();
        let restore = saved.clone();
        let default_panic = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            Guard::restore(&restore);
            default_panic(info);
        }));
        Some(Guard { saved })
    }

    fn restore(saved: &str) {
        print!("\x1b[?2004l\x1b[0m\x1b[?25h\x1b[?1049l");
        let _ = std::io::stdout().flush();
        let _ = stty(&[saved]);
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        Guard::restore(&self.saved);
    }
}

// ---------------------------------------------------------------------------
// input
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum Key {
    Char(char),
    Ctrl(char),
    Enter,
    Esc,
    Tab,
    BackTab,
    Backspace,
    Delete,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Paste(String),
}

/// How long a lone ESC byte waits for the rest of an escape sequence before
/// it counts as the Esc key.
const ESC_WAIT: Duration = Duration::from_millis(30);

pub struct Input {
    rx: mpsc::Receiver<u8>,
    pending: Vec<u8>,
    stalled_since: Option<Instant>,
}

impl Input {
    pub fn spawn() -> Input {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut stdin = std::io::stdin().lock();
            let mut buf = [0u8; 256];
            while let Ok(n) = stdin.read(&mut buf) {
                if n == 0 || buf[..n].iter().any(|&b| tx.send(b).is_err()) {
                    break;
                }
            }
        });
        Input { rx, pending: Vec::new(), stalled_since: None }
    }

    /// Waits up to `timeout` for keys. Returns whatever is complete.
    pub fn wait(&mut self, timeout: Duration) -> Vec<Key> {
        let timeout = if self.pending.is_empty() { timeout } else { ESC_WAIT };
        if let Ok(b) = self.rx.recv_timeout(timeout) {
            self.pending.push(b);
        }
        while let Ok(b) = self.rx.try_recv() {
            self.pending.push(b);
        }
        let mut keys = parse(&mut self.pending);
        if self.pending.is_empty() {
            self.stalled_since = None;
        } else {
            let since = *self.stalled_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= ESC_WAIT && !self.pending.starts_with(PASTE_START) {
                // A bare Esc, or bytes that will never complete; drop one
                // byte and carry on.
                if self.pending.remove(0) == 0x1b {
                    keys.push(Key::Esc);
                }
                keys.extend(parse(&mut self.pending));
                self.stalled_since = None;
            }
        }
        keys
    }
}

const PASTE_START: &[u8] = b"\x1b[200~";
const PASTE_END: &[u8] = b"\x1b[201~";

/// Parses complete keys off the front of `buf`, leaving any incomplete tail.
pub fn parse(buf: &mut Vec<u8>) -> Vec<Key> {
    let mut keys = Vec::new();
    let mut i = 0;
    while i < buf.len() {
        let rest = &buf[i..];
        let (key, used) = match rest[0] {
            0x1b => match parse_escape(rest) {
                Some(found) => found,
                None => break, // incomplete
            },
            b'\r' | b'\n' => (Some(Key::Enter), 1),
            b'\t' => (Some(Key::Tab), 1),
            0x7f | 0x08 => (Some(Key::Backspace), 1),
            b @ 0x01..=0x1a => (Some(Key::Ctrl((b + 0x60) as char)), 1),
            b if b < 0x20 => (None, 1),
            b => {
                let len = match b {
                    0xc0..=0xdf => 2,
                    0xe0..=0xef => 3,
                    0xf0..=0xf7 => 4,
                    0x80..=0xbf | 0xf8..=0xff => {
                        i += 1; // stray continuation byte
                        continue;
                    }
                    _ => 1,
                };
                if rest.len() < len {
                    break;
                }
                match std::str::from_utf8(&rest[..len]) {
                    Ok(s) => (s.chars().next().map(Key::Char), len),
                    Err(_) => (None, 1),
                }
            }
        };
        keys.extend(key);
        i += used;
    }
    buf.drain(..i);
    keys
}

/// `None` means "need more bytes".
fn parse_escape(s: &[u8]) -> Option<(Option<Key>, usize)> {
    match s.get(1)? {
        b'[' => {
            if s.starts_with(PASTE_START) {
                let body = &s[PASTE_START.len()..];
                let end = body.windows(PASTE_END.len()).position(|w| w == PASTE_END)?;
                let text = String::from_utf8_lossy(&body[..end]).into_owned();
                return Some((Some(Key::Paste(text)), PASTE_START.len() + end + PASTE_END.len()));
            }
            // CSI: parameters 0x30–0x3f, intermediates 0x20–0x2f, final 0x40–0x7e
            let fin = s[2..].iter().position(|&b| (0x40..=0x7e).contains(&b))? + 2;
            let params = std::str::from_utf8(&s[2..fin]).unwrap_or("");
            let first = params.split(';').next().unwrap_or("");
            let key = match s[fin] {
                b'A' => Some(Key::Up),
                b'B' => Some(Key::Down),
                b'C' => Some(Key::Right),
                b'D' => Some(Key::Left),
                b'H' => Some(Key::Home),
                b'F' => Some(Key::End),
                b'Z' => Some(Key::BackTab),
                b'~' => match first {
                    "1" | "7" => Some(Key::Home),
                    "4" | "8" => Some(Key::End),
                    "3" => Some(Key::Delete),
                    "5" => Some(Key::PageUp),
                    "6" => Some(Key::PageDown),
                    _ => None,
                },
                _ => None,
            };
            Some((key, fin + 1))
        }
        b'O' => {
            let key = match s.get(2)? {
                b'A' => Some(Key::Up),
                b'B' => Some(Key::Down),
                b'C' => Some(Key::Right),
                b'D' => Some(Key::Left),
                b'H' => Some(Key::Home),
                b'F' => Some(Key::End),
                _ => None,
            };
            Some((key, 3))
        }
        // Esc followed by something else (Alt+key, or two fast keys).
        _ => Some((Some(Key::Esc), 1)),
    }
}

// ---------------------------------------------------------------------------
// screen
// ---------------------------------------------------------------------------

pub struct Screen {
    out: String,
    fg: Option<Rgb>,
    bg: Option<Rgb>,
    strike: bool,
    bold: bool,
}

impl Screen {
    pub fn new() -> Screen {
        Screen { out: String::with_capacity(32 * 1024), fg: None, bg: None, strike: false, bold: false }
    }

    /// Starts a frame: home the cursor and paint every cell `bg`.
    pub fn begin(&mut self, bg: Rgb) {
        self.out.clear();
        self.fg = None;
        self.bg = None;
        self.strike = false;
        self.bold = false;
        self.out.push_str("\x1b[0m\x1b[H");
        self.bg(bg);
        self.out.push_str("\x1b[2J");
    }

    pub fn fg(&mut self, c: Rgb) -> &mut Self {
        if self.fg != Some(c) {
            self.fg = Some(c);
            self.out.push_str(&format!("\x1b[38;2;{};{};{}m", c.0, c.1, c.2));
        }
        self
    }

    pub fn bg(&mut self, c: Rgb) -> &mut Self {
        if self.bg != Some(c) {
            self.bg = Some(c);
            self.out.push_str(&format!("\x1b[48;2;{};{};{}m", c.0, c.1, c.2));
        }
        self
    }

    pub fn strike(&mut self, on: bool) -> &mut Self {
        if self.strike != on {
            self.strike = on;
            self.out.push_str(if on { "\x1b[9m" } else { "\x1b[29m" });
        }
        self
    }

    pub fn bold(&mut self, on: bool) -> &mut Self {
        if self.bold != on {
            self.bold = on;
            self.out.push_str(if on { "\x1b[1m" } else { "\x1b[22m" });
        }
        self
    }

    pub fn at(&mut self, row: usize, col: usize) -> &mut Self {
        self.out.push_str(&format!("\x1b[{};{}H", row + 1, col + 1));
        self
    }

    pub fn text(&mut self, s: &str) -> &mut Self {
        self.out.push_str(s);
        self
    }

    /// Fills `n` cells with the current background.
    pub fn blank(&mut self, n: usize) -> &mut Self {
        for _ in 0..n {
            self.out.push(' ');
        }
        self
    }

    pub fn flush(&mut self) {
        self.out.push_str("\x1b[0m");
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(self.out.as_bytes());
        let _ = out.flush();
    }
}

/// Display width, counting each char as one cell (good enough for to-dos).
pub fn width(s: &str) -> usize {
    s.chars().count()
}

/// Cuts `s` to at most `w` cells, ending with `…` if it was cut.
pub fn fit(s: &str, w: usize) -> String {
    if width(s) <= w {
        return s.to_string();
    }
    if w == 0 {
        return String::new();
    }
    let mut out: String = s.chars().take(w - 1).collect();
    out.push('…');
    out
}

// ---------------------------------------------------------------------------
// single-line text editor
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct LineEdit {
    chars: Vec<char>,
    pos: usize,
}

impl LineEdit {
    pub fn new(text: &str) -> LineEdit {
        let chars: Vec<char> = text.chars().collect();
        LineEdit { pos: chars.len(), chars }
    }

    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.chars.iter().all(|c| c.is_whitespace())
    }

    /// Returns false for keys the editor does not handle.
    pub fn key(&mut self, key: &Key) -> bool {
        match key {
            Key::Char(c) => {
                self.chars.insert(self.pos, *c);
                self.pos += 1;
            }
            Key::Paste(s) => {
                for c in s.chars().map(|c| if c.is_control() { ' ' } else { c }) {
                    self.chars.insert(self.pos, c);
                    self.pos += 1;
                }
            }
            Key::Backspace | Key::Ctrl('h') if self.pos > 0 => {
                self.pos -= 1;
                self.chars.remove(self.pos);
            }
            Key::Delete | Key::Ctrl('d') if self.pos < self.chars.len() => {
                self.chars.remove(self.pos);
            }
            Key::Left | Key::Ctrl('b') => self.pos = self.pos.saturating_sub(1),
            Key::Right | Key::Ctrl('f') => self.pos = (self.pos + 1).min(self.chars.len()),
            Key::Home | Key::Ctrl('a') => self.pos = 0,
            Key::End | Key::Ctrl('e') => self.pos = self.chars.len(),
            Key::Ctrl('u') => {
                self.chars.drain(..self.pos);
                self.pos = 0;
            }
            Key::Ctrl('k') => self.chars.truncate(self.pos),
            Key::Ctrl('w') => {
                let mut start = self.pos;
                while start > 0 && self.chars[start - 1] == ' ' {
                    start -= 1;
                }
                while start > 0 && self.chars[start - 1] != ' ' {
                    start -= 1;
                }
                self.chars.drain(start..self.pos);
                self.pos = start;
            }
            Key::Backspace | Key::Delete | Key::Ctrl('h') | Key::Ctrl('d') => {}
            _ => return false,
        }
        true
    }

    /// Draws the text in `w` cells with a block cursor, scrolling so the
    /// cursor stays visible.
    pub fn draw(&self, s: &mut Screen, w: usize, fg: Rgb, bg: Rgb) {
        if w == 0 {
            return;
        }
        let start = (self.pos + 1).saturating_sub(w);
        let mut used = 0;
        for (n, c) in self.chars.iter().enumerate().skip(start) {
            if used + 1 > w {
                break;
            }
            if n == self.pos {
                s.fg(bg).bg(fg).text(&c.to_string()).fg(fg).bg(bg);
            } else {
                s.fg(fg).bg(bg).text(&c.to_string());
            }
            used += 1;
        }
        if self.pos == self.chars.len() && used < w {
            s.bg(fg).text(" ").bg(bg);
            used += 1;
        }
        s.blank(w - used);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(bytes: &[u8]) -> (Vec<Key>, Vec<u8>) {
        let mut buf = bytes.to_vec();
        let k = parse(&mut buf);
        (k, buf)
    }

    #[test]
    fn parses_plain_and_utf8() {
        assert_eq!(keys(b"ax").0, vec![Key::Char('a'), Key::Char('x')]);
        assert_eq!(keys("é✓".as_bytes()).0, vec![Key::Char('é'), Key::Char('✓')]);
        // split multi-byte char waits for the rest
        let (k, rest) = keys(&"✓".as_bytes()[..2]);
        assert!(k.is_empty());
        assert_eq!(rest.len(), 2);
    }

    #[test]
    fn parses_control_and_escapes() {
        assert_eq!(
            keys(b"\r\t\x7f\x03\x1b[A\x1b[B\x1bOC\x1b[3~\x1b[Z\x1b[1;5D\x1b[5~").0,
            vec![
                Key::Enter,
                Key::Tab,
                Key::Backspace,
                Key::Ctrl('c'),
                Key::Up,
                Key::Down,
                Key::Right,
                Key::Delete,
                Key::BackTab,
                Key::Left,
                Key::PageUp
            ]
        );
    }

    #[test]
    fn lone_esc_waits_and_paste_is_one_key() {
        let (k, rest) = keys(b"\x1b");
        assert!(k.is_empty());
        assert_eq!(rest, b"\x1b");
        assert_eq!(keys(b"\x1bj").0, vec![Key::Esc, Key::Char('j')]);
        assert_eq!(
            keys(b"\x1b[200~eggs\nmilk\x1b[201~q").0,
            vec![Key::Paste("eggs\nmilk".into()), Key::Char('q')]
        );
        let (k, rest) = keys(b"\x1b[200~half");
        assert!(k.is_empty());
        assert_eq!(rest.len(), 10);
    }

    #[test]
    fn line_edit_basics() {
        let mut e = LineEdit::new("buy milk");
        e.key(&Key::Ctrl('w'));
        assert_eq!(e.text(), "buy ");
        e.key(&Key::Paste("oat\nmilk".into()));
        assert_eq!(e.text(), "buy oat milk");
        e.key(&Key::Home);
        e.key(&Key::Delete);
        e.key(&Key::Char('B'));
        e.key(&Key::Right);
        e.key(&Key::Backspace);
        assert_eq!(e.text(), "By oat milk");
        e.key(&Key::Ctrl('k'));
        assert_eq!(e.text(), "B");
        e.key(&Key::End);
        e.key(&Key::Ctrl('u'));
        assert!(e.is_empty());
        assert!(!e.key(&Key::Enter));
    }

    #[test]
    fn fit_truncates() {
        assert_eq!(fit("hello", 5), "hello");
        assert_eq!(fit("hello", 4), "hel…");
        assert_eq!(fit("hello", 0), "");
    }
}
