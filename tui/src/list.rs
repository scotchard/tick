//! The full-screen list: tabs for lists, one row per to-do, vim-ish keys.

use std::time::{Duration, Instant};

use tick_core::{display_path, local_date, Item, List, Span, Store, Theme, ThemeWatch, INBOX};

use crate::term::{self, fit, width, Guard, Input, Key, LineEdit, Screen};

const POLL: Duration = Duration::from_millis(500);
const MSG_TTL: Duration = Duration::from_secs(4);
const DATE_FMT: &str = "%a %b %-d";

enum Mode {
    Normal,
    Add(LineEdit),
    Edit(usize, LineEdit),
    NewList(LineEdit),
    Rename(LineEdit),
    /// Picking the list to move the current item to.
    Move(usize),
    ConfirmDeleteList,
    Help,
}

struct Msg {
    text: String,
    error: bool,
    at: Instant,
}

pub struct App {
    store: Store,
    watch: ThemeWatch,
    theme: Theme,
    list: usize,
    cursor: usize,
    scroll: usize,
    mode: Mode,
    msg: Option<Msg>,
    date: String,
    date_at: Instant,
    size: (usize, usize),
    quit: bool,
}

pub fn run(store: Store) -> i32 {
    let Some(_guard) = Guard::enter("Tick") else {
        eprintln!("tick: needs a terminal (stdin is not a tty). Try `tick ls` or `tick add`.");
        return 1;
    };
    let mut input = Input::spawn();
    let mut screen = Screen::new();
    let mut app = App::new(store);
    let mut last_poll = Instant::now();
    app.draw(&mut screen);
    while !app.quit {
        let keys = input.wait(POLL);
        let mut dirty = !keys.is_empty();
        for key in keys {
            app.key(key);
            if app.quit {
                return 0;
            }
        }
        if last_poll.elapsed() >= POLL {
            last_poll = Instant::now();
            dirty |= app.poll();
        }
        if dirty {
            app.draw(&mut screen);
        }
    }
    0
}

impl App {
    fn new(store: Store) -> App {
        let watch = ThemeWatch::new();
        App {
            theme: watch.theme(),
            watch,
            store,
            list: 0,
            cursor: 0,
            scroll: 0,
            mode: Mode::Normal,
            msg: None,
            date: local_date(DATE_FMT),
            date_at: Instant::now(),
            size: term::size().unwrap_or((24, 80)),
            quit: false,
        }
    }

    // -- state helpers -----------------------------------------------------

    fn current(&self) -> Option<&List> {
        self.store.doc.lists.get(self.list)
    }

    fn current_item(&self) -> Option<&Item> {
        self.current()?.item(self.cursor)
    }

    fn clamp(&mut self) {
        let lists = self.store.doc.lists.len();
        self.list = self.list.min(lists.saturating_sub(1));
        let len = self.current().map_or(0, List::len);
        self.cursor = self.cursor.min(len.saturating_sub(1));
    }

    fn say(&mut self, text: impl Into<String>) {
        self.msg = Some(Msg { text: text.into(), error: false, at: Instant::now() });
    }

    fn fail(&mut self, text: impl Into<String>) {
        self.msg = Some(Msg { text: text.into(), error: true, at: Instant::now() });
    }

    /// Runs an edit through the store; reports save errors in the footer.
    fn edit<R>(&mut self, f: impl FnOnce(&mut tick_core::Doc) -> R) -> Option<R> {
        let out = match self.store.edit(f) {
            Ok(r) => Some(r),
            Err(e) => {
                self.fail(format!("Couldn't save {}: {e}", display_path(self.store.path())));
                None
            }
        };
        self.clamp();
        out
    }

    fn body_height(&self) -> usize {
        self.size.0.saturating_sub(4).max(1)
    }

    /// Periodic checks. Returns true if anything visible changed.
    fn poll(&mut self) -> bool {
        let mut dirty = false;
        if let Some(size) = term::size() {
            if size != self.size {
                self.size = size;
                dirty = true;
            }
        }
        if let Some(theme) = self.watch.poll() {
            self.theme = theme;
            dirty = true;
        }
        // Don't yank the list out from under an edit in progress; the store
        // reloads before saving anyway.
        if matches!(self.mode, Mode::Normal | Mode::Help) {
            match self.store.refresh() {
                Ok(true) => {
                    self.clamp();
                    self.say("Reloaded: the file changed outside Tick");
                    dirty = true;
                }
                Ok(false) => {}
                Err(e) => {
                    self.fail(format!("Couldn't read {}: {e}", display_path(self.store.path())));
                    dirty = true;
                }
            }
        }
        if self.date_at.elapsed() >= Duration::from_secs(60) {
            self.date_at = Instant::now();
            let date = local_date(DATE_FMT);
            if date != self.date {
                self.date = date;
                dirty = true;
            }
        }
        if self.msg.as_ref().is_some_and(|m| m.at.elapsed() >= MSG_TTL) {
            self.msg = None;
            dirty = true;
        }
        dirty
    }

    // -- keys ----------------------------------------------------------------

    fn key(&mut self, key: Key) {
        let mode = std::mem::replace(&mut self.mode, Mode::Normal);
        self.mode = match mode {
            Mode::Normal => {
                self.normal_key(key);
                return;
            }
            Mode::Help => Mode::Normal,
            Mode::Add(mut line) => match key {
                Key::Esc | Key::Ctrl('c') => Mode::Normal,
                Key::Enter if line.is_empty() => Mode::Normal,
                Key::Enter => {
                    let text = line.text();
                    let list = self.list;
                    let added = self.edit(|d| {
                        let list = if d.lists.is_empty() { d.ensure(INBOX) } else { list };
                        d.lists[list].add(Item::new(&text))
                    });
                    if let Some(i) = added {
                        self.cursor = i;
                    }
                    Mode::Add(LineEdit::default())
                }
                k => {
                    line.key(&k);
                    Mode::Add(line)
                }
            },
            Mode::Edit(i, mut line) => match key {
                Key::Esc | Key::Ctrl('c') => Mode::Normal,
                Key::Enter => {
                    if !line.is_empty() {
                        let text = line.text();
                        let list = self.list;
                        self.edit(|d| {
                            if let Some(it) = d.lists.get_mut(list).and_then(|l| l.item_mut(i)) {
                                it.text = tick_core::doc::clean(&text);
                            }
                        });
                    }
                    Mode::Normal
                }
                k => {
                    line.key(&k);
                    Mode::Edit(i, line)
                }
            },
            Mode::NewList(mut line) => match key {
                Key::Esc | Key::Ctrl('c') => Mode::Normal,
                Key::Enter => {
                    let name = tick_core::doc::clean(&line.text());
                    if name.is_empty() {
                        Mode::Normal
                    } else if self.store.doc.find(&name).is_some() {
                        self.fail(format!("There's already a list called {name}"));
                        Mode::NewList(line)
                    } else {
                        if let Some(i) = self.edit(|d| d.ensure(&name)) {
                            self.list = i;
                            self.cursor = 0;
                        }
                        Mode::Normal
                    }
                }
                k => {
                    line.key(&k);
                    Mode::NewList(line)
                }
            },
            Mode::Rename(mut line) => match key {
                Key::Esc | Key::Ctrl('c') => Mode::Normal,
                Key::Enter => {
                    let name = tick_core::doc::clean(&line.text());
                    let list = self.list;
                    if name.is_empty() {
                        Mode::Normal
                    } else if self.store.doc.find(&name).is_some_and(|i| i != list) {
                        self.fail(format!("There's already a list called {name}"));
                        Mode::Rename(line)
                    } else {
                        self.edit(|d| d.lists[list].name = name);
                        Mode::Normal
                    }
                }
                k => {
                    line.key(&k);
                    Mode::Rename(line)
                }
            },
            Mode::Move(to) => {
                let n = self.store.doc.lists.len();
                let step = |mut t: usize, fwd: bool, skip: usize| {
                    loop {
                        t = if fwd { (t + 1) % n } else { (t + n - 1) % n };
                        if t != skip {
                            return t;
                        }
                    }
                };
                match key {
                    Key::Esc | Key::Ctrl('c') | Key::Char('q') => Mode::Normal,
                    Key::Right | Key::Tab | Key::Char('l') | Key::Char('m') => Mode::Move(step(to, true, self.list)),
                    Key::Left | Key::BackTab | Key::Char('h') => Mode::Move(step(to, false, self.list)),
                    Key::Char(c @ '1'..='9') if (c as usize - '1' as usize) < n && (c as usize - '1' as usize) != self.list => {
                        Mode::Move(c as usize - '1' as usize)
                    }
                    Key::Enter => {
                        let (from, i) = (self.list, self.cursor);
                        let name = self.store.doc.lists[to].name.clone();
                        if self.edit(|d| d.move_item(from, i, to)).flatten().is_some() {
                            self.say(format!("Moved to {name}"));
                        }
                        Mode::Normal
                    }
                    _ => Mode::Move(to),
                }
            }
            Mode::ConfirmDeleteList => {
                if key == Key::Char('y') || key == Key::Char('Y') {
                    self.delete_list();
                }
                Mode::Normal
            }
        };
    }

    fn normal_key(&mut self, key: Key) {
        let len = self.current().map_or(0, List::len);
        let lists = self.store.doc.lists.len();
        match key {
            Key::Char('q') | Key::Ctrl('c') => self.quit = true,
            Key::Char('j') | Key::Down => self.cursor = (self.cursor + 1).min(len.saturating_sub(1)),
            Key::Char('k') | Key::Up => self.cursor = self.cursor.saturating_sub(1),
            Key::Char('g') | Key::Home => self.cursor = 0,
            Key::Char('G') | Key::End => self.cursor = len.saturating_sub(1),
            Key::PageDown | Key::Ctrl('d') => {
                self.cursor = (self.cursor + self.body_height()).min(len.saturating_sub(1))
            }
            Key::PageUp | Key::Ctrl('u') => self.cursor = self.cursor.saturating_sub(self.body_height()),
            Key::Tab | Key::Char('l') | Key::Right if lists > 0 => self.switch((self.list + 1) % lists),
            Key::BackTab | Key::Char('h') | Key::Left if lists > 0 => self.switch((self.list + lists - 1) % lists),
            Key::Char(c @ '1'..='9') if (c as usize - '1' as usize) < lists => self.switch(c as usize - '1' as usize),
            Key::Char('x') | Key::Char(' ') if len > 0 => {
                let (list, i) = (self.list, self.cursor);
                self.edit(|d| {
                    if let Some(it) = d.lists[list].item_mut(i) {
                        it.done = !it.done;
                    }
                });
            }
            Key::Char('a') | Key::Char('o') => self.mode = Mode::Add(LineEdit::default()),
            Key::Char('e') | Key::Char('i') | Key::Enter => {
                if let Some(it) = self.current_item() {
                    self.mode = Mode::Edit(self.cursor, LineEdit::new(&it.text));
                }
            }
            Key::Char('d') | Key::Delete if len > 0 => {
                let (list, i) = (self.list, self.cursor);
                if let Some(Some(it)) = self.edit(|d| d.lists[list].remove(i)) {
                    self.say(format!("Deleted “{}” · u to undo", fit(&it.text, 40)));
                }
            }
            Key::Char('J') if len > 0 => {
                let (list, i) = (self.list, self.cursor);
                if let Some(j) = self.edit(|d| d.lists[list].shift(i, false)) {
                    self.cursor = j;
                }
            }
            Key::Char('K') if len > 0 => {
                let (list, i) = (self.list, self.cursor);
                if let Some(j) = self.edit(|d| d.lists[list].shift(i, true)) {
                    self.cursor = j;
                }
            }
            Key::Char('m') if len > 0 => {
                if lists < 2 {
                    self.fail("Make another list first (N)");
                } else {
                    self.mode = Mode::Move((self.list + 1) % lists);
                }
            }
            Key::Char('C') if len > 0 => {
                let list = self.list;
                match self.edit(|d| d.lists[list].clear_done()) {
                    Some(0) => self.say("No done items to clear"),
                    Some(n) => self.say(format!("Cleared {n} done · u to undo")),
                    None => {}
                }
            }
            Key::Char('N') => self.mode = Mode::NewList(LineEdit::default()),
            Key::Char('R') => {
                if let Some(l) = self.current() {
                    self.mode = Mode::Rename(LineEdit::new(&l.name));
                }
            }
            Key::Char('D') if lists > 0 => {
                if len == 0 {
                    self.delete_list();
                } else {
                    self.mode = Mode::ConfirmDeleteList;
                }
            }
            Key::Char('u') => match self.store.undo() {
                Ok(true) => {
                    self.clamp();
                    self.say("Undone");
                }
                Ok(false) => self.say("Nothing to undo"),
                Err(e) => self.fail(format!("Couldn't undo: {e}")),
            },
            Key::Char('?') => self.mode = Mode::Help,
            _ => {}
        }
    }

    fn switch(&mut self, list: usize) {
        if list != self.list {
            self.list = list;
            self.cursor = 0;
            self.scroll = 0;
        }
    }

    fn delete_list(&mut self) {
        let list = self.list;
        if let Some(l) = self.edit(|d| d.lists.remove(list)) {
            self.say(format!("Deleted list {} · u to undo", l.name));
        }
    }

    // -- drawing -------------------------------------------------------------

    fn draw(&mut self, s: &mut Screen) {
        let t = self.theme;
        let (rows, cols) = self.size;
        s.begin(t.bg);
        if rows < 5 || cols < 24 {
            s.at(0, 0).fg(t.dim).text("Tick: window too small");
            s.flush();
            return;
        }
        let w = cols - 2;
        let rule = "─".repeat(w);
        self.draw_tabs(s, w);
        s.at(1, 1).fg(t.muted).bg(t.bg).text(&rule);
        self.draw_body(s, 2, rows - 4, w);
        s.at(rows - 2, 1).fg(t.muted).bg(t.bg).text(&rule);
        self.draw_footer(s, rows - 1, w);
        if matches!(self.mode, Mode::Help) {
            self.draw_help(s, rows, cols);
        }
        s.flush();
    }

    fn draw_tabs(&self, s: &mut Screen, w: usize) {
        let t = self.theme;
        let lists = &self.store.doc.lists;
        s.at(0, 1);
        if lists.is_empty() {
            s.fg(t.dim).text("Tick");
            return;
        }
        let labels: Vec<String> = lists
            .iter()
            .map(|l| match l.open_count() {
                0 => format!(" {} ", l.name),
                n => format!(" {} {n} ", l.name),
            })
            .collect();
        let widths: Vec<usize> = labels.iter().map(|l| width(l)).collect();
        let total: usize = widths.iter().sum::<usize>() + labels.len().saturating_sub(1);
        let show_date = !self.date.is_empty() && total + width(&self.date) + 2 <= w;

        // Scroll the tab strip so the current tab is visible.
        let mut start = 0;
        if total > w {
            let room = w.saturating_sub(2);
            while start < self.list
                && widths[start..=self.list].iter().sum::<usize>() + (self.list - start) > room
            {
                start += 1;
            }
            if start > 0 {
                s.fg(t.dim).text("‹ ");
            }
        }
        let mut used = if start > 0 { 2 } else { 0 };
        for (i, (label, l)) in labels.iter().zip(lists).enumerate().skip(start) {
            let gap = usize::from(i > start);
            if used + gap + widths[i] > w {
                break;
            }
            s.bg(t.bg).text(&" ".repeat(gap));
            if i == self.list {
                s.bg(t.accent).fg(t.bg).bold(true).text(label).bold(false).bg(t.bg);
            } else {
                let count = l.open_count();
                s.fg(t.dim).text(&format!(" {}", l.name));
                if count > 0 {
                    s.fg(t.muted).text(&format!(" {count}"));
                }
                s.text(" ");
            }
            used += gap + widths[i];
        }
        if show_date {
            s.at(0, 1 + w - width(&self.date)).fg(t.dim).bg(t.bg).text(&self.date);
        }
    }

    fn draw_body(&mut self, s: &mut Screen, top: usize, height: usize, w: usize) {
        let t = self.theme;
        let Some(list) = self.store.doc.lists.get(self.list) else {
            s.at(top + 1, 3).fg(t.dim).text(&fit("No lists yet. Press N to make one, or a to add a to-do.", w - 2));
            return;
        };
        let items: Vec<&Item> = list.items().collect();

        // Rows to show: every item, plus the editor line while adding.
        let mut rows: Vec<Option<usize>> = (0..items.len()).map(Some).collect();
        let focus = if let Mode::Add(_) = self.mode {
            let at = items.iter().rposition(|i| !i.done).map_or(0, |p| p + 1);
            rows.insert(at, None);
            at
        } else {
            self.cursor
        };
        if focus < self.scroll {
            self.scroll = focus;
        } else if focus >= self.scroll + height {
            self.scroll = focus + 1 - height;
        }
        self.scroll = self.scroll.min(rows.len().saturating_sub(height));

        if rows.is_empty() {
            s.at(top + 1, 3).fg(t.dim).text(&fit("Nothing here yet. Press a to add a to-do.", w - 2));
            return;
        }

        let text_w = w.saturating_sub(6);
        for (r, row) in rows.iter().enumerate().skip(self.scroll).take(height) {
            let y = top + r - self.scroll;
            match row {
                None => {
                    s.at(y, 1).bg(t.bg).fg(t.accent).text("› [ ] ");
                    if let Mode::Add(line) = &self.mode {
                        line.draw(s, text_w, t.fg, t.bg);
                    }
                }
                Some(i) => {
                    let it = items[*i];
                    let selected = *i == self.cursor && !matches!(self.mode, Mode::Add(_));
                    let bg = if selected { t.selection } else { t.bg };
                    s.at(y, 0).bg(bg).blank(w + 2);
                    s.at(y, 1).fg(t.accent).text(if selected { "›" } else { " " }).text(" ");
                    if it.done {
                        s.fg(t.green).text("[x]");
                    } else {
                        s.fg(t.dim).text("[ ]");
                    }
                    s.text(" ");
                    if let Mode::Edit(ei, line) = &self.mode {
                        if *ei == *i {
                            line.draw(s, text_w, t.fg, bg);
                            continue;
                        }
                    }
                    draw_spans(s, &it.text, text_w, it.done, &t);
                    s.strike(false).bg(t.bg);
                }
            }
        }
        if self.scroll > 0 {
            s.at(top, 1 + w - 1).fg(t.dim).bg(t.bg).text("↑");
        }
        if self.scroll + height < rows.len() {
            s.at(top + height - 1, 1 + w - 1).fg(t.dim).bg(t.bg).text("↓");
        }
    }

    fn draw_footer(&self, s: &mut Screen, y: usize, w: usize) {
        let t = self.theme;
        let hints: &[(&str, &str)] = match self.mode {
            Mode::Normal => &[
                ("a", "add"),
                ("x", "tick"),
                ("e", "edit"),
                ("d", "delete"),
                ("m", "move"),
                ("?", "help"),
                ("q", "quit"),
            ],
            Mode::Add(_) => &[("↵", "add"), ("esc", "done")],
            Mode::Edit(..) | Mode::NewList(_) | Mode::Rename(_) => &[("↵", "save"), ("esc", "cancel")],
            Mode::Move(_) => &[("←→", "pick"), ("↵", "move"), ("esc", "cancel")],
            Mode::ConfirmDeleteList => &[("y", "delete"), ("any key", "keep")],
            Mode::Help => &[("any key", "close")],
        };

        // Left side.
        s.at(y, 1).bg(t.bg);
        let mut left = 0;
        match &self.mode {
            Mode::NewList(line) | Mode::Rename(line) => {
                let label = if matches!(self.mode, Mode::NewList(_)) { "New list: " } else { "Rename list: " };
                s.fg(t.accent).text(label);
                let room = w.saturating_sub(width(label) + 18).max(8);
                line.draw(s, room, t.fg, t.bg);
                left = width(label) + room;
            }
            Mode::Move(to) => {
                s.fg(t.dim).text("Move to:");
                left = 8;
                for (i, l) in self.store.doc.lists.iter().enumerate() {
                    if i == self.list {
                        continue;
                    }
                    let label = format!(" {} ", l.name);
                    if left + 1 + width(&label) > w.saturating_sub(30) {
                        s.fg(t.dim).text(" …");
                        left += 2;
                        break;
                    }
                    s.text(" ");
                    if i == *to {
                        s.bg(t.accent).fg(t.bg).text(&label).bg(t.bg);
                    } else {
                        s.fg(t.fg).text(&label);
                    }
                    left += 1 + width(&label);
                }
            }
            Mode::ConfirmDeleteList => {
                let l = &self.store.doc.lists[self.list];
                let q = fit(&format!("Delete the list “{}” and its {} to-dos?", l.name, l.len()), w.saturating_sub(24));
                s.fg(t.red).text(&q);
                left = width(&q);
            }
            _ => {
                if let Some(m) = &self.msg {
                    let text = fit(&m.text, w.saturating_sub(10));
                    s.fg(if m.error { t.red } else { t.fg }).text(&text);
                    left = width(&text);
                } else if let Some(l) = self.current() {
                    let open = l.open_count();
                    let text = format!("{open} open · {} done", l.len() - open);
                    s.fg(t.dim).text(&text);
                    left = width(&text);
                }
            }
        }

        // Right side: as many hints as fit.
        let mut shown = hints.len();
        let hint_w = |n: usize| -> usize {
            hints[..n].iter().map(|(k, l)| width(k) + 1 + width(l)).sum::<usize>() + n.saturating_sub(1) * 2
        };
        while shown > 0 && left + 2 + hint_w(shown) > w {
            shown -= 1;
        }
        if shown > 0 {
            s.at(y, 1 + w - hint_w(shown));
            for (n, (k, l)) in hints[..shown].iter().enumerate() {
                if n > 0 {
                    s.text("  ");
                }
                s.fg(t.accent).text(k).fg(t.dim).text(" ").text(l);
            }
        }
    }

    fn draw_help(&self, s: &mut Screen, rows: usize, cols: usize) {
        let t = self.theme;
        const KEYS: &[(&str, &str)] = &[
            ("j k ↑ ↓", "move the cursor"),
            ("tab h l", "switch list (1–9 jumps)"),
            ("x space", "tick / untick"),
            ("a", "add to-dos (↵ adds another)"),
            ("e ↵", "edit"),
            ("d", "delete"),
            ("J K", "move item down / up"),
            ("m", "move item to another list"),
            ("C", "clear done items"),
            ("N R D", "new / rename / delete list"),
            ("u", "undo"),
            ("q", "quit"),
        ];
        let path = display_path(self.store.path());
        let bw = cols.saturating_sub(4).clamp(20, 54);
        // Leave the footer visible below the box.
        let bh = (KEYS.len() + 5).min(rows.saturating_sub(2));
        let x = (cols - bw) / 2;
        let y = (rows - 1 - bh) / 2;
        for r in 0..bh {
            s.at(y + r, x).bg(t.raised).blank(bw);
        }
        s.at(y + 1, x + 2).fg(t.fg).bold(true).text("Keys").bold(false);
        for (n, (k, d)) in KEYS.iter().enumerate() {
            if y + 2 + n >= y + bh - 2 {
                break;
            }
            s.at(y + 2 + n, x + 2).fg(t.accent).text(&format!("{k:<10}"));
            s.fg(t.fg).text(&fit(d, bw.saturating_sub(14)));
        }
        s.at(y + bh - 2, x + 2).fg(t.dim).text(&fit(&path, bw - 4));
    }
}

/// Item text with `#tags` and `due:` dates coloured, cut to `w` cells.
fn draw_spans(s: &mut Screen, text: &str, w: usize, done: bool, t: &Theme) {
    let mut room = w;
    s.strike(done);
    for span in tick_core::doc::spans(text) {
        let (str, color) = match span {
            Span::Plain(p) => (p, if done { t.dim } else { t.fg }),
            Span::Tag(p) => (p, if done { t.dim } else { t.cyan }),
            Span::Due(p) => (p, if done { t.dim } else { t.yellow }),
        };
        let piece = fit(str, room);
        s.fg(color).text(&piece);
        room -= width(&piece);
        if width(&piece) < width(str) || room == 0 {
            break;
        }
    }
}
