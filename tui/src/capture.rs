//! Quick capture: a one-line box meant for a small floating terminal window.
//! Type, Enter, gone. Tab picks the list.

use std::time::{Duration, Instant};

use tick_core::{display_path, Item, Store, Theme, ThemeWatch, INBOX};

use crate::term::{self, fit, width, Guard, Input, Key, LineEdit, Screen};

const POLL: Duration = Duration::from_millis(500);

pub fn run(mut store: Store, list: Option<String>) -> i32 {
    let Some(guard) = Guard::enter("Tick capture") else {
        eprintln!("tick: capture needs a terminal. Use `tick add \"…\"` from scripts.");
        return 1;
    };
    let wanted = list.unwrap_or_else(|| INBOX.to_string());

    // Choices are the file's lists, plus the requested one if it doesn't
    // exist yet (it's created on save).
    let mut choices: Vec<String> = store.doc.lists.iter().map(|l| l.name.clone()).collect();
    let mut target = match store.doc.find(&wanted) {
        Some(i) => i,
        None => {
            choices.push(wanted.clone());
            choices.len() - 1
        }
    };

    let mut watch = ThemeWatch::new();
    let mut theme = watch.theme();
    let mut input = Input::spawn();
    let mut screen = Screen::new();
    let mut line = LineEdit::default();
    let mut error: Option<String> = None;
    let mut size = term::size().unwrap_or((5, 60));
    let mut last_poll = Instant::now();
    draw(&mut screen, &theme, size, &line, &choices[target], error.as_deref());

    loop {
        let keys = input.wait(POLL);
        let mut dirty = !keys.is_empty();
        for key in keys {
            match key {
                Key::Esc | Key::Ctrl('c') => return 0,
                Key::Enter if line.is_empty() => return 0,
                Key::Enter => {
                    let text = line.text();
                    let name = choices[target].clone();
                    match store.edit(|d| {
                        let i = d.ensure(&name);
                        d.lists[i].add(Item::new(&text));
                    }) {
                        Ok(()) => {
                            drop(guard);
                            return 0;
                        }
                        Err(e) => error = Some(format!("Couldn't save {}: {e}", display_path(store.path()))),
                    }
                }
                Key::Tab => target = (target + 1) % choices.len(),
                Key::BackTab => target = (target + choices.len() - 1) % choices.len(),
                k => {
                    line.key(&k);
                    error = None;
                }
            }
        }
        if last_poll.elapsed() >= POLL {
            last_poll = Instant::now();
            if let Some(t) = watch.poll() {
                theme = t;
                dirty = true;
            }
            if let Some(s) = term::size().filter(|s| *s != size) {
                size = s;
                dirty = true;
            }
        }
        if dirty {
            draw(&mut screen, &theme, size, &line, &choices[target], error.as_deref());
        }
    }
}

fn draw(s: &mut Screen, t: &Theme, (rows, cols): (usize, usize), line: &LineEdit, list: &str, error: Option<&str>) {
    s.begin(t.bg);
    if cols < 12 || rows == 0 {
        s.flush();
        return;
    }
    let w = cols - 2;
    let top = rows.saturating_sub(3) / 2;

    // Input row.
    s.at(top, 1).fg(t.accent).bold(true).text("+").bold(false).text(" ");
    let room = w - 2;
    if line.is_empty() && line.text().is_empty() {
        s.bg(t.fg).text(" ").bg(t.bg);
        s.fg(t.muted).text(&fit(&format!("New to-do for {list}"), room.saturating_sub(1)));
    } else {
        line.draw(s, room, t.fg, t.bg);
    }
    if rows < 3 {
        s.flush();
        return;
    }

    s.at(top + 1, 1).fg(t.raised).text(&"─".repeat(w));

    // Hint row: where it goes, then keys.
    s.at(top + 2, 1);
    if let Some(e) = error {
        s.fg(t.red).text(&fit(e, w));
    } else {
        let parts: [(&str, String); 3] =
            [("↵", format!("add to {list}")), ("tab", "pick list".into()), ("esc", "cancel".into())];
        let mut used = 0;
        for (n, (k, label)) in parts.iter().enumerate() {
            let piece_w = width(k) + 1 + width(label) + if n > 0 { 3 } else { 0 };
            if used + piece_w > w {
                break;
            }
            if n > 0 {
                s.text("   ");
            }
            s.fg(t.fg).bold(true).text(k).bold(false).fg(t.dim).text(" ");
            if n == 0 {
                s.text("add to ").fg(t.accent).text(list);
            } else {
                s.text(label);
            }
            used += piece_w;
        }
    }
    s.flush();
}
