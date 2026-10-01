//! `tick-gui`: Tick as a native window, in the spirit of Omawrite. A calm
//! single column (the "sheet"), an optional lists sidebar, and colours that
//! follow the current Omarchy theme live.

use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use eframe::egui::{
    self, pos2, text::LayoutJob, text::TextFormat, text::TextWrapping, vec2, Align, Align2, Color32, CornerRadius,
    FontData, FontDefinitions, FontFamily, FontId, Frame, Id, Key, Layout, Margin, Modifiers, Rect, RichText, Sense,
    Shadow, Stroke, StrokeKind, TextEdit, ViewportCommand,
};
use tick_core::store::Stamp;
use tick_core::{display_path, local_date, Item, Rgb, Span, Store, Theme, ThemeWatch, INBOX};

const COLUMN: f32 = 640.0;
const ROW_H: f32 = 36.0;
const BODY: f32 = 15.0;
const SIDEBAR_W: f32 = 210.0;
const MSG_TTL: Duration = Duration::from_secs(4);
const DATE_FMT: &str = "%A · %b %-d";

const USAGE: &str = "\
tick-gui: Tick as a window.

Usage: tick-gui [-f PATH] [--app-id ID]

  -f, --file PATH   use PATH instead of $TICK_FILE or ~/Documents/Tick/todo.md
      --app-id ID   Wayland app id for window rules (default: tick)
  -h, --help        show this help";

fn main() {
    let mut args = std::env::args().skip(1);
    let mut file: Option<PathBuf> = None;
    let mut app_id = "tick".to_string();
    while let Some(a) = args.next() {
        match a.as_str() {
            "-f" | "--file" => file = args.next().map(PathBuf::from),
            "--app-id" => app_id = args.next().unwrap_or(app_id),
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            _ if a.starts_with("--file=") => file = Some(PathBuf::from(&a[7..])),
            _ => {
                eprintln!("tick-gui: unknown argument `{a}`\n\n{USAGE}");
                std::process::exit(2);
            }
        }
    }
    let path = file.unwrap_or_else(tick_core::default_path);
    let store = match Store::open(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("tick-gui: couldn't open {}: {e}", display_path(&path));
            std::process::exit(1);
        }
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Tick")
            .with_app_id(app_id)
            .with_inner_size([720.0, 680.0])
            .with_min_inner_size([360.0, 320.0]),
        ..Default::default()
    };
    let result = eframe::run_native(
        "Tick",
        options,
        Box::new(move |cc| {
            install_fonts(&cc.egui_ctx);
            Ok(Box::new(App::new(&cc.egui_ctx, store)))
        }),
    );
    if let Err(e) = result {
        eprintln!("tick-gui: {e}");
        std::process::exit(1);
    }
}

fn c(rgb: Rgb) -> Color32 {
    Color32::from_rgb(rgb.0, rgb.1, rgb.2)
}

// ---------------------------------------------------------------------------
// fonts and theme
// ---------------------------------------------------------------------------

fn bold() -> FontFamily {
    FontFamily::Name("bold".into())
}

/// iA Writer Mono, the face Omawrite ships, with egui's defaults behind it
/// for emoji and other scripts.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "ia".into(),
        Arc::new(FontData::from_static(include_bytes!("../fonts/iAWriterMonoS-Regular.ttf"))),
    );
    fonts.font_data.insert(
        "ia-bold".into(),
        Arc::new(FontData::from_static(include_bytes!("../fonts/iAWriterMonoS-Bold.ttf"))),
    );
    let fallbacks: Vec<String> = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for fam in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts.families.entry(fam).or_default().insert(0, "ia".into());
    }
    let mut b = vec!["ia-bold".to_string()];
    b.extend(fallbacks);
    fonts.families.insert(bold(), b);
    ctx.set_fonts(fonts);
}

fn apply_theme(ctx: &egui::Context, t: &Theme) {
    let mut v = if t.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.dark_mode = t.dark;
    v.override_text_color = Some(c(t.fg));
    v.panel_fill = c(t.bg);
    v.window_fill = c(t.raised);
    v.window_stroke = Stroke::new(1.0, c(t.muted));
    v.window_corner_radius = CornerRadius::ZERO;
    v.menu_corner_radius = CornerRadius::ZERO;
    v.window_shadow = Shadow::NONE;
    v.popup_shadow = Shadow::NONE;
    v.extreme_bg_color = c(t.panel);
    v.faint_bg_color = c(t.panel);
    v.code_bg_color = c(t.panel);
    v.text_edit_bg_color = Some(c(t.bg));
    v.selection.bg_fill = c(t.selection);
    v.selection.stroke = Stroke::new(1.0, c(t.fg));
    v.text_cursor.stroke = Stroke::new(2.0, c(t.accent));
    // A blinking caret repaints twice a second forever; keep idle at zero.
    v.text_cursor.blink = false;
    v.hyperlink_color = c(t.accent);
    v.warn_fg_color = c(t.yellow);
    v.error_fg_color = c(t.red);
    let w = &mut v.widgets;
    for (wv, fill) in [
        (&mut w.noninteractive, t.raised),
        (&mut w.inactive, t.raised),
        (&mut w.hovered, t.selection),
        (&mut w.active, t.selection),
        (&mut w.open, t.selection),
    ] {
        wv.bg_fill = c(fill);
        wv.weak_bg_fill = c(fill);
        wv.corner_radius = CornerRadius::ZERO;
        wv.fg_stroke = Stroke::new(1.0, c(t.fg));
        wv.bg_stroke = Stroke::NONE;
    }
    w.noninteractive.bg_stroke = Stroke::new(1.0, c(t.selection));
    ctx.all_styles_mut(|s| {
        s.visuals = v.clone();
        s.spacing.item_spacing = vec2(8.0, 4.0);
        s.spacing.button_padding = vec2(10.0, 5.0);
        s.spacing.menu_margin = Margin::same(6);
        s.text_styles.insert(egui::TextStyle::Body, FontId::proportional(BODY));
        s.text_styles.insert(egui::TextStyle::Button, FontId::proportional(14.0));
        s.text_styles.insert(egui::TextStyle::Small, FontId::proportional(12.0));
        s.text_styles.insert(egui::TextStyle::Heading, FontId::new(26.0, bold()));
    });
}

// ---------------------------------------------------------------------------
// background watcher: theme file, to-do file, date
// ---------------------------------------------------------------------------

enum Event {
    Theme(Theme),
    Disk,
    Date(String),
}

/// Stats a couple of files twice a second and wakes the UI only when
/// something changed, so an idle window draws nothing.
fn spawn_watcher(ctx: egui::Context, path: PathBuf) -> mpsc::Receiver<Event> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut theme = ThemeWatch::new();
        let mut stamp = Stamp::of(&path);
        let mut date = local_date(DATE_FMT);
        let mut ticks = 0u32;
        loop {
            std::thread::sleep(Duration::from_millis(500));
            let mut events = Vec::new();
            if let Some(t) = theme.poll() {
                events.push(Event::Theme(t));
            }
            let now = Stamp::of(&path);
            if now != stamp {
                stamp = now;
                events.push(Event::Disk);
            }
            ticks += 1;
            if ticks.is_multiple_of(60) {
                let d = local_date(DATE_FMT);
                if d != date {
                    date = d.clone();
                    events.push(Event::Date(d));
                }
            }
            if events.is_empty() {
                continue;
            }
            for e in events {
                if tx.send(e).is_err() {
                    return;
                }
            }
            ctx.request_repaint();
        }
    });
    rx
}

// ---------------------------------------------------------------------------
// small UI state file (sidebar on/off, last list)
// ---------------------------------------------------------------------------

fn state_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(base.join("tick/gui"))
}

fn load_state() -> (bool, String) {
    let text = state_path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    let mut sidebar = true;
    let mut list = String::new();
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("sidebar=") {
            sidebar = v.trim() != "0";
        } else if let Some(v) = line.strip_prefix("list=") {
            list = v.to_string();
        }
    }
    (sidebar, list)
}

fn save_state(sidebar: bool, list: &str) {
    if let Some(p) = state_path() {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, format!("sidebar={}\nlist={list}\n", u8::from(sidebar)));
    }
}

// ---------------------------------------------------------------------------
// app
// ---------------------------------------------------------------------------

enum Action {
    Toggle(usize),
    Delete(usize),
    StartEdit(usize),
    CommitEdit,
    CancelEdit,
    MoveTo(usize, usize),
    Add(String),
    Select(Option<usize>),
    Shift(usize, bool),
    SwitchList(usize),
    ClearDone(usize),
    AskDeleteList(usize),
    DeleteList(usize),
    StartRename(usize),
    StartNewList,
    CommitListEdit,
    CancelListEdit,
    Undo,
    ToggleSidebar,
    FocusAdd,
    Help,
}

struct ItemEdit {
    index: usize,
    text: String,
    fresh: bool,
}

enum ListEdit {
    New(String, bool),
    Rename(usize, String, bool),
}

struct App {
    store: Store,
    theme: Theme,
    events: mpsc::Receiver<Event>,
    date: String,
    list: usize,
    selected: Option<usize>,
    editing: Option<ItemEdit>,
    adding: String,
    focus_add: bool,
    sidebar: bool,
    list_edit: Option<ListEdit>,
    confirm_delete: Option<usize>,
    help: bool,
    msg: Option<(String, bool, Instant)>,
}

impl App {
    fn new(ctx: &egui::Context, store: Store) -> App {
        let theme = Theme::load();
        apply_theme(ctx, &theme);
        let (sidebar, last) = load_state();
        let list = store.doc.find(&last).unwrap_or(0);
        App {
            events: spawn_watcher(ctx.clone(), store.path().to_path_buf()),
            store,
            theme,
            date: local_date(DATE_FMT),
            list,
            selected: None,
            editing: None,
            adding: String::new(),
            focus_add: true,
            sidebar,
            list_edit: None,
            confirm_delete: None,
            help: false,
            msg: None,
        }
    }

    fn say(&mut self, text: impl Into<String>) {
        self.msg = Some((text.into(), false, Instant::now()));
    }

    fn fail(&mut self, text: impl Into<String>) {
        self.msg = Some((text.into(), true, Instant::now()));
    }

    fn list_len(&self) -> usize {
        self.store.doc.lists.get(self.list).map_or(0, |l| l.len())
    }

    fn clamp(&mut self) {
        let n = self.store.doc.lists.len();
        self.list = self.list.min(n.saturating_sub(1));
        let len = self.list_len();
        if self.selected.is_some_and(|s| s >= len) {
            self.selected = len.checked_sub(1);
        }
        if self.editing.as_ref().is_some_and(|e| e.index >= len) {
            self.editing = None;
        }
    }

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

    fn save_ui_state(&self) {
        let name = self.store.doc.lists.get(self.list).map_or("", |l| l.name.as_str());
        save_state(self.sidebar, name);
    }

    fn drain_events(&mut self, ctx: &egui::Context) {
        while let Ok(e) = self.events.try_recv() {
            match e {
                Event::Theme(t) => {
                    self.theme = t;
                    apply_theme(ctx, &t);
                }
                Event::Date(d) => self.date = d,
                // Don't reload under an edit in progress; saving reloads first anyway.
                Event::Disk if self.editing.is_none() && self.list_edit.is_none() => {
                    match self.store.refresh() {
                        Ok(true) => {
                            self.clamp();
                            self.say("Reloaded: the file changed outside Tick");
                        }
                        Ok(false) => {}
                        Err(e) => self.fail(format!("Couldn't read {}: {e}", display_path(self.store.path()))),
                    }
                }
                Event::Disk => {}
            }
        }
    }

    fn apply(&mut self, action: Action) {
        let list = self.list;
        match action {
            Action::Toggle(i) => {
                self.edit(|d| {
                    if let Some(it) = d.lists.get_mut(list).and_then(|l| l.item_mut(i)) {
                        it.done = !it.done;
                    }
                });
            }
            Action::Delete(i) => {
                if let Some(Some(it)) = self.edit(|d| d.lists.get_mut(list).and_then(|l| l.remove(i))) {
                    self.say(format!("Deleted “{}” · Ctrl+Z to undo", it.text));
                    self.editing = None;
                }
            }
            Action::StartEdit(i) => {
                if let Some(it) = self.store.doc.lists.get(list).and_then(|l| l.item(i)) {
                    self.editing = Some(ItemEdit { index: i, text: it.text.clone(), fresh: true });
                    self.selected = Some(i);
                }
            }
            Action::CommitEdit => {
                if let Some(e) = self.editing.take() {
                    if !e.text.trim().is_empty() {
                        self.edit(|d| {
                            if let Some(it) = d.lists.get_mut(list).and_then(|l| l.item_mut(e.index)) {
                                it.text = tick_core::doc::clean(&e.text);
                            }
                        });
                    }
                }
            }
            Action::CancelEdit => self.editing = None,
            Action::MoveTo(i, to) => {
                let name = self.store.doc.lists.get(to).map(|l| l.name.clone()).unwrap_or_default();
                if self.edit(|d| d.move_item(list, i, to)).flatten().is_some() {
                    self.say(format!("Moved to {name}"));
                }
            }
            Action::Add(text) => {
                let added = self.edit(|d| {
                    let l = if d.lists.is_empty() { d.ensure(INBOX) } else { list };
                    d.lists[l].add(Item::new(&text))
                });
                if added.is_some() {
                    self.selected = None;
                }
            }
            Action::Select(s) => self.selected = s,
            Action::Shift(i, up) => {
                if let Some(j) = self.edit(|d| d.lists.get_mut(list).map(|l| l.shift(i, up))).flatten() {
                    self.selected = Some(j);
                }
            }
            Action::SwitchList(i) => {
                if i < self.store.doc.lists.len() && i != self.list {
                    self.list = i;
                    self.selected = None;
                    self.editing = None;
                    self.save_ui_state();
                }
            }
            Action::ClearDone(l) => match self.edit(|d| d.lists.get_mut(l).map_or(0, |l| l.clear_done())) {
                Some(0) => self.say("No done items to clear"),
                Some(n) => self.say(format!("Cleared {n} done · Ctrl+Z to undo")),
                None => {}
            },
            Action::AskDeleteList(l) => {
                if self.store.doc.lists.get(l).is_some_and(|l| l.is_empty()) {
                    self.apply(Action::DeleteList(l));
                } else {
                    self.confirm_delete = Some(l);
                }
            }
            Action::DeleteList(l) => {
                if let Some(removed) = self.edit(|d| (l < d.lists.len()).then(|| d.lists.remove(l))).flatten() {
                    self.say(format!("Deleted list {} · Ctrl+Z to undo", removed.name));
                    if self.list > l {
                        self.list -= 1;
                    }
                    self.clamp();
                    self.selected = None;
                    self.save_ui_state();
                }
            }
            Action::StartRename(l) => {
                if let Some(name) = self.store.doc.lists.get(l).map(|l| l.name.clone()) {
                    self.list_edit = Some(ListEdit::Rename(l, name, true));
                    self.sidebar = true;
                }
            }
            Action::StartNewList => {
                self.list_edit = Some(ListEdit::New(String::new(), true));
                self.sidebar = true;
            }
            Action::CommitListEdit => match self.list_edit.take() {
                Some(ListEdit::New(name, _)) => {
                    let name = tick_core::doc::clean(&name);
                    if name.is_empty() {
                    } else if self.store.doc.find(&name).is_some() {
                        self.fail(format!("There's already a list called {name}"));
                    } else if let Some(i) = self.edit(|d| d.ensure(&name)) {
                        self.apply(Action::SwitchList(i));
                        self.focus_add = true;
                    }
                }
                Some(ListEdit::Rename(l, name, _)) => {
                    let name = tick_core::doc::clean(&name);
                    if name.is_empty() {
                    } else if self.store.doc.find(&name).is_some_and(|i| i != l) {
                        self.fail(format!("There's already a list called {name}"));
                    } else {
                        self.edit(|d| {
                            if let Some(list) = d.lists.get_mut(l) {
                                list.name = name;
                            }
                        });
                        self.save_ui_state();
                    }
                }
                None => {}
            },
            Action::CancelListEdit => self.list_edit = None,
            Action::Undo => match self.store.undo() {
                Ok(true) => {
                    self.clamp();
                    self.editing = None;
                    self.say("Undone");
                }
                Ok(false) => self.say("Nothing to undo"),
                Err(e) => self.fail(format!("Couldn't undo: {e}")),
            },
            Action::ToggleSidebar => {
                self.sidebar = !self.sidebar;
                self.save_ui_state();
            }
            Action::FocusAdd => self.focus_add = true,
            Action::Help => self.help = !self.help,
        }
    }

    // -- keyboard ------------------------------------------------------------

    fn shortcuts(&mut self, ctx: &egui::Context) -> Vec<Action> {
        let mut out = Vec::new();
        let ctrl = Modifiers::COMMAND;
        let ctrl_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        let n = self.store.doc.lists.len();
        let typing = ctx.egui_wants_keyboard_input();
        ctx.input_mut(|i| {
            if i.consume_key(ctrl, Key::Q) || i.consume_key(ctrl, Key::W) {
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            if i.consume_key(ctrl, Key::B) {
                out.push(Action::ToggleSidebar);
            }
            if i.consume_key(ctrl_shift, Key::N) {
                out.push(Action::StartNewList);
            } else if i.consume_key(ctrl, Key::N) {
                out.push(Action::FocusAdd);
            }
            if n > 0 {
                if i.consume_key(ctrl, Key::PageDown) || i.consume_key(ctrl, Key::Tab) {
                    out.push(Action::SwitchList((self.list + 1) % n));
                }
                if i.consume_key(ctrl, Key::PageUp) || i.consume_key(ctrl_shift, Key::Tab) {
                    out.push(Action::SwitchList((self.list + n - 1) % n));
                }
                let nums = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];
                for (k, key) in nums.into_iter().enumerate() {
                    if k < n && i.consume_key(ctrl, key) {
                        out.push(Action::SwitchList(k));
                    }
                }
            }
            if i.consume_key(Modifiers::NONE, Key::F1) {
                out.push(Action::Help);
            }
            if typing || self.help || self.confirm_delete.is_some() {
                return;
            }

            // Single keys, only when no text field has focus.
            let len = self.list_len();
            let sel = self.selected;
            let plain = Modifiers::NONE;
            if i.consume_key(ctrl, Key::Z) || i.consume_key(plain, Key::U) {
                out.push(Action::Undo);
            }
            if i.consume_key(plain, Key::ArrowDown) || i.consume_key(plain, Key::J) {
                out.push(Action::Select(Some(sel.map_or(0, |s| (s + 1).min(len.saturating_sub(1))))));
            }
            if i.consume_key(plain, Key::ArrowUp) || i.consume_key(plain, Key::K) {
                out.push(Action::Select(Some(sel.map_or(len.saturating_sub(1), |s| s.saturating_sub(1)))));
            }
            if i.consume_key(Modifiers::SHIFT, Key::Questionmark) || i.consume_key(plain, Key::Questionmark) {
                out.push(Action::Help);
            }
            if i.consume_key(plain, Key::A) || i.consume_key(plain, Key::O) {
                out.push(Action::FocusAdd);
            }
            if i.consume_key(plain, Key::Escape) {
                out.push(Action::Select(None));
            }
            let Some(s) = sel.filter(|&s| s < len) else { return };
            if i.consume_key(plain, Key::Space) || i.consume_key(plain, Key::X) {
                out.push(Action::Toggle(s));
            }
            if i.consume_key(plain, Key::Enter) || i.consume_key(plain, Key::E) {
                out.push(Action::StartEdit(s));
            }
            if i.consume_key(plain, Key::Delete) || i.consume_key(plain, Key::D) {
                out.push(Action::Delete(s));
            }
            if i.consume_key(Modifiers::ALT, Key::ArrowUp) || i.consume_key(Modifiers::SHIFT, Key::K) {
                out.push(Action::Shift(s, true));
            }
            if i.consume_key(Modifiers::ALT, Key::ArrowDown) || i.consume_key(Modifiers::SHIFT, Key::J) {
                out.push(Action::Shift(s, false));
            }
        });
        out
    }

    // -- sidebar (look C) ------------------------------------------------------

    fn sidebar_ui(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let t = self.theme;
        let w = ui.available_width();
        let lists: Vec<(String, usize)> =
            self.store.doc.lists.iter().map(|l| (l.name.clone(), l.open_count())).collect();
        for (i, (name, open)) in lists.iter().enumerate() {
            if let Some(ListEdit::Rename(l, buf, fresh)) = &mut self.list_edit {
                if *l == i {
                    let (rect, _) = ui.allocate_exact_size(vec2(w, 32.0), Sense::hover());
                    ui.painter().rect_filled(rect, 0, c(t.raised));
                    ui.painter().rect_filled(Rect::from_min_size(rect.min, vec2(2.0, rect.height())), 0, c(t.accent));
                    let font = FontId::proportional(14.0);
                    let r = ui.put(
                        field_rect(ui, rect, rect.left() + 18.0, rect.right() - 18.0, &font),
                        TextEdit::singleline(buf).frame(Frame::NONE).margin(Margin::ZERO).font(font),
                    );
                    if std::mem::take(fresh) {
                        r.request_focus();
                    }
                    list_edit_keys(ui, &r, actions);
                    continue;
                }
            }
            let (rect, resp) = ui.allocate_exact_size(vec2(w, 32.0), Sense::click());
            let on = i == self.list;
            if on {
                ui.painter().rect_filled(rect, 0, c(t.raised));
                ui.painter().rect_filled(Rect::from_min_size(rect.min, vec2(2.0, rect.height())), 0, c(t.accent));
            } else if resp.hovered() {
                ui.painter().rect_filled(rect, 0, c(t.raised).gamma_multiply(0.5));
            }
            let font = FontId::proportional(14.0);
            let count_w = if *open > 0 { 36.0 } else { 0.0 };
            let name_g = one_line(ui, name, font.clone(), c(if on { t.fg } else { t.dim }), w - 36.0 - count_w);
            ui.painter().galley(pos2(rect.left() + 18.0, rect.center().y - name_g.size().y / 2.0), name_g, c(t.fg));
            if *open > 0 {
                ui.painter().text(
                    pos2(rect.right() - 16.0, rect.center().y),
                    Align2::RIGHT_CENTER,
                    open,
                    font,
                    c(if on { t.dim } else { t.muted }),
                );
            }
            if resp.clicked() {
                actions.push(Action::SwitchList(i));
            }
            resp.context_menu(|ui| list_menu(ui, i, actions));
        }

        ui.add_space(10.0);
        let y = ui.cursor().top();
        ui.painter().hline(ui.max_rect().x_range().shrink(16.0), y, Stroke::new(1.0, c(t.raised)));
        ui.add_space(10.0);

        if let Some(ListEdit::New(buf, fresh)) = &mut self.list_edit {
            let (rect, _) = ui.allocate_exact_size(vec2(w, 32.0), Sense::hover());
            let font = FontId::proportional(14.0);
            let r = ui.put(
                field_rect(ui, rect, rect.left() + 18.0, rect.right() - 18.0, &font),
                TextEdit::singleline(buf)
                    .frame(Frame::NONE)
                    .margin(Margin::ZERO)
                    .font(font)
                    .hint_text(RichText::new("List name").color(c(t.muted))),
            );
            if std::mem::take(fresh) {
                r.request_focus();
            }
            list_edit_keys(ui, &r, actions);
        } else {
            let (rect, resp) = ui.allocate_exact_size(vec2(w, 32.0), Sense::click());
            let color = if resp.hovered() { t.fg } else { t.dim };
            ui.painter().text(
                pos2(rect.left() + 18.0, rect.center().y),
                Align2::LEFT_CENTER,
                "+ New list",
                FontId::proportional(14.0),
                c(color),
            );
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                actions.push(Action::StartNewList);
            }
        }
    }

    // -- sheet (look B) ----------------------------------------------------------

    fn sheet_ui(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            let avail = ui.available_width();
            let w = (avail - 64.0).clamp(220.0, COLUMN);
            let pad = ((avail - w) / 2.0).max(0.0);
            ui.add_space(40.0);
            ui.horizontal_top(|ui| {
                ui.add_space(pad);
                ui.vertical(|ui| {
                    ui.set_width(w);
                    self.sheet_column(ui, w, actions);
                });
            });
            ui.add_space(32.0);
        });
    }

    fn sheet_column(&mut self, ui: &mut egui::Ui, w: f32, actions: &mut Vec<Action>) {
        let t = self.theme;
        ui.label(RichText::new(self.date.to_uppercase()).size(12.0).color(c(t.dim)));
        ui.add_space(2.0);

        let lists: Vec<String> = self.store.doc.lists.iter().map(|l| l.name.clone()).collect();
        let title = lists.get(self.list).cloned().unwrap_or_else(|| "Tick".into());
        let title_resp = ui
            .add(egui::Label::new(RichText::new(&title).font(FontId::new(26.0, bold())).color(c(t.fg))).sense(Sense::click()))
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Lists");
        let current = self.list;
        let sidebar = self.sidebar;
        egui::Popup::menu(&title_resp).show(|ui| {
            ui.set_min_width(200.0);
            for (i, name) in lists.iter().enumerate() {
                let label = if i == current { RichText::new(name).color(c(t.accent)) } else { RichText::new(name) };
                if ui.button(label).clicked() {
                    actions.push(Action::SwitchList(i));
                }
            }
            ui.separator();
            if ui.button("New list…").clicked() {
                actions.push(Action::StartNewList);
            }
            if !lists.is_empty() {
                list_menu(ui, current, actions);
            }
            ui.separator();
            if ui.button(if sidebar { "Hide sidebar" } else { "Show sidebar" }).clicked() {
                actions.push(Action::ToggleSidebar);
            }
        });
        ui.add_space(16.0);

        let items: Vec<Item> =
            self.store.doc.lists.get(self.list).map(|l| l.items().cloned().collect()).unwrap_or_default();
        for (i, item) in items.iter().enumerate() {
            self.item_row(ui, i, item, w, &lists, actions);
        }
        self.add_row(ui, w, actions);
    }

    fn item_row(&mut self, ui: &mut egui::Ui, i: usize, item: &Item, w: f32, lists: &[String], actions: &mut Vec<Action>) {
        let t = self.theme;
        let (rect, resp) = ui.allocate_exact_size(vec2(w, ROW_H), Sense::click());
        let id = Id::new(("item", self.list, i));
        let painter = ui.painter().clone();
        let selected = self.selected == Some(i);
        let editing = self.editing.as_ref().is_some_and(|e| e.index == i);
        let bg_rect = rect.expand2(vec2(10.0, 0.0));
        if selected || editing {
            painter.rect_filled(bg_rect, 0, c(t.raised));
        } else if resp.hovered() {
            painter.rect_filled(bg_rect, 0, c(t.raised).gamma_multiply(0.45));
        }

        // Checkbox.
        let box_rect = Rect::from_center_size(pos2(rect.left() + 8.0, rect.center().y), vec2(15.0, 15.0));
        let box_resp = ui
            .interact(box_rect.expand(7.0), id.with("box"), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if item.done {
            painter.rect_filled(box_rect, 0, c(t.green));
            let (l, cy) = (box_rect.left(), box_rect.center().y);
            let stroke = Stroke::new(2.0, c(t.bg));
            painter.line_segment([pos2(l + 3.2, cy + 0.2), pos2(l + 6.2, cy + 3.2)], stroke);
            painter.line_segment([pos2(l + 6.2, cy + 3.2), pos2(l + 12.0, cy - 3.4)], stroke);
        } else {
            let col = if box_resp.hovered() { t.accent } else { t.dim };
            painter.rect_stroke(box_rect, 0, Stroke::new(1.5, c(col)), StrokeKind::Inside);
        }

        // Delete button, shown on hover.
        let del_rect = Rect::from_center_size(pos2(rect.right() - 8.0, rect.center().y), vec2(20.0, 20.0));
        let del_resp = ui.interact(del_rect, id.with("del"), Sense::click());
        let show_del = !editing && (resp.hovered() || del_resp.hovered());
        if show_del {
            let col = if del_resp.hovered() { t.red } else { t.muted };
            let (cx, cy, r) = (del_rect.center().x, del_rect.center().y, 4.0);
            let stroke = Stroke::new(1.5, c(col));
            painter.line_segment([pos2(cx - r, cy - r), pos2(cx + r, cy + r)], stroke);
            painter.line_segment([pos2(cx - r, cy + r), pos2(cx + r, cy - r)], stroke);
        }

        let text_rect = Rect::from_min_max(pos2(rect.left() + 30.0, rect.top()), pos2(rect.right() - 24.0, rect.bottom()));
        if let Some(e) = self.editing.as_mut().filter(|e| e.index == i) {
            let font = FontId::proportional(BODY);
            let r = ui.put(
                field_rect(ui, rect, text_rect.left(), text_rect.right(), &font),
                TextEdit::singleline(&mut e.text)
                    .id(id.with("edit"))
                    .frame(Frame::NONE)
                    .margin(Margin::ZERO)
                    .font(FontId::proportional(BODY))
                    .text_color(c(t.fg)),
            );
            if std::mem::take(&mut e.fresh) {
                r.request_focus();
            }
            if r.lost_focus() {
                if ui.input(|inp| inp.key_pressed(Key::Escape)) {
                    actions.push(Action::CancelEdit);
                } else {
                    actions.push(Action::CommitEdit);
                }
            }
        } else {
            let galley = ui.fonts_mut(|f| f.layout_job(item_job(&item.text, item.done, text_rect.width(), &t)));
            painter.galley(pos2(text_rect.left(), rect.center().y - galley.size().y / 2.0), galley, c(t.fg));
        }

        if box_resp.clicked() {
            actions.push(Action::Toggle(i));
        } else if show_del && del_resp.clicked() {
            actions.push(Action::Delete(i));
        } else if resp.clicked() && !editing {
            actions.push(Action::StartEdit(i));
        }
        resp.context_menu(|ui| {
            if ui.button(if item.done { "Mark as not done" } else { "Mark as done" }).clicked() {
                actions.push(Action::Toggle(i));
            }
            if ui.button("Edit").clicked() {
                actions.push(Action::StartEdit(i));
            }
            if lists.len() > 1 {
                ui.menu_button("Move to", |ui| {
                    for (l, name) in lists.iter().enumerate() {
                        if l != self.list && ui.button(name).clicked() {
                            actions.push(Action::MoveTo(i, l));
                        }
                    }
                });
            }
            ui.separator();
            if ui.button(RichText::new("Delete").color(c(t.red))).clicked() {
                actions.push(Action::Delete(i));
            }
        });
    }

    fn add_row(&mut self, ui: &mut egui::Ui, w: f32, actions: &mut Vec<Action>) {
        let t = self.theme;
        let (rect, _) = ui.allocate_exact_size(vec2(w, ROW_H), Sense::hover());
        let id = Id::new("add-todo");
        let focused = ui.memory(|m| m.has_focus(id));
        ui.painter().text(
            pos2(rect.left() + 8.0, rect.center().y),
            Align2::CENTER_CENTER,
            "+",
            FontId::proportional(BODY + 2.0),
            c(if focused { t.accent } else { t.muted }),
        );
        let font = FontId::proportional(BODY);
        let r = ui.put(
            field_rect(ui, rect, rect.left() + 30.0, rect.right() - 24.0, &font),
            TextEdit::singleline(&mut self.adding)
                .id(id)
                .frame(Frame::NONE)
                .margin(Margin::ZERO)
                .font(FontId::proportional(BODY))
                .text_color(c(t.fg))
                .hint_text(RichText::new("Add a to-do").color(c(t.muted))),
        );
        if std::mem::take(&mut self.focus_add) {
            r.request_focus();
        }
        if r.lost_focus() {
            let (enter, esc) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
            if enter && !self.adding.trim().is_empty() {
                actions.push(Action::Add(std::mem::take(&mut self.adding)));
                actions.push(Action::FocusAdd); // keep typing the next one
            } else if esc {
                self.adding.clear();
            }
        }
    }

    // -- footer ------------------------------------------------------------------

    fn footer_ui(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let t = self.theme;
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icon_button(ui, &t, Icon::Help).on_hover_text("Shortcuts (F1)").clicked() {
                    actions.push(Action::Help);
                }
                if icon_button(ui, &t, Icon::Sidebar).on_hover_text("Toggle sidebar (Ctrl+B)").clicked() {
                    actions.push(Action::ToggleSidebar);
                }
                ui.add_space(6.0);
                if let Some(l) = self.store.doc.lists.get(self.list) {
                    let open = l.open_count();
                    let done = l.len() - open;
                    if done > 0 {
                        let r = ui
                            .add(
                                egui::Label::new(RichText::new(format!("{done} done")).size(12.0).color(c(t.dim)))
                                    .sense(Sense::click()),
                            )
                            .on_hover_text("Click to clear done items")
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if r.clicked() {
                            actions.push(Action::ClearDone(self.list));
                        }
                        ui.label(RichText::new("·").size(12.0).color(c(t.muted)));
                    }
                    ui.label(RichText::new(format!("{open} open")).size(12.0).color(c(t.dim)));
                }
                ui.add_space(16.0);
                // Message or file path gets whatever width is left.
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    let (text, color) = match &self.msg {
                        Some((text, err, _)) => (text.clone(), if *err { t.red } else { t.fg }),
                        None => (display_path(self.store.path()), t.dim),
                    };
                    ui.add(egui::Label::new(RichText::new(text).size(12.0).color(c(color))).truncate());
                });
            });
        });
    }

    fn overlays(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let t = self.theme;
        let frame = Frame::NONE.fill(c(t.raised)).inner_margin(Margin::same(24)).stroke(Stroke::new(1.0, c(t.selection)));
        let backdrop = Color32::from_black_alpha(if t.dark { 120 } else { 60 });
        if self.help {
            let r = egui::Modal::new(Id::new("help")).frame(frame).backdrop_color(backdrop).show(ctx, |ui| {
                ui.label(RichText::new("Shortcuts").font(FontId::new(17.0, bold())));
                ui.add_space(10.0);
                egui::Grid::new("keys").num_columns(2).spacing(vec2(24.0, 6.0)).show(ui, |ui| {
                    for (k, d) in SHORTCUTS {
                        ui.label(RichText::new(*k).color(c(t.accent)).size(13.0));
                        ui.label(RichText::new(*d).size(13.0));
                        ui.end_row();
                    }
                });
                ui.add_space(10.0);
                ui.label(RichText::new(display_path(self.store.path())).size(12.0).color(c(t.dim)));
            });
            if r.should_close() {
                self.help = false;
            }
        }
        if let Some(l) = self.confirm_delete {
            let (name, n) = self.store.doc.lists.get(l).map(|l| (l.name.clone(), l.len())).unwrap_or_default();
            let mut close = false;
            let r = egui::Modal::new(Id::new("confirm-delete")).frame(frame).backdrop_color(backdrop).show(ctx, |ui| {
                ui.set_max_width(360.0);
                ui.label(RichText::new(format!("Delete the list “{name}”?")).font(FontId::new(17.0, bold())));
                ui.add_space(6.0);
                ui.label(RichText::new(format!("Its {n} to-dos go with it. Ctrl+Z brings them back.")).color(c(t.dim)));
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("Delete list").color(c(t.red))).clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                        actions.push(Action::DeleteList(l));
                        close = true;
                    }
                    if ui.button("Keep").clicked() {
                        close = true;
                    }
                });
            });
            if close || r.should_close() {
                self.confirm_delete = None;
            }
        }
    }
}

const SHORTCUTS: &[(&str, &str)] = &[
    ("Ctrl+N  a", "add a to-do"),
    ("↑ ↓  j k", "select"),
    ("Space  x", "tick / untick"),
    ("Enter  e", "edit (or click the text)"),
    ("Delete  d", "delete"),
    ("Alt+↑ ↓  K J", "move up / down"),
    ("Ctrl+Tab  Ctrl+1–9", "switch list"),
    ("Ctrl+Shift+N", "new list"),
    ("Ctrl+B", "toggle sidebar"),
    ("Ctrl+Z  u", "undo"),
    ("Right-click", "more options"),
    ("Ctrl+Q", "quit"),
];

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.drain_events(&ctx);
        let mut actions = self.shortcuts(&ctx);
        let t = self.theme;

        if self.sidebar {
            egui::Panel::left("lists")
                .exact_size(SIDEBAR_W)
                .resizable(false)
                .frame(Frame::NONE.fill(c(t.panel)).inner_margin(Margin { left: 0, right: 0, top: 28, bottom: 16 }))
                .show(ui, |ui| self.sidebar_ui(ui, &mut actions));
        }
        egui::Panel::bottom("footer")
            .frame(Frame::NONE.fill(c(t.bg)).inner_margin(Margin::symmetric(20, 10)))
            .show(ui, |ui| self.footer_ui(ui, &mut actions));
        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(c(t.bg)))
            .show(ui, |ui| self.sheet_ui(ui, &mut actions));
        self.overlays(&ctx, &mut actions);

        for a in actions {
            self.apply(a);
        }

        // Clear the footer message when it expires.
        if let Some((_, _, at)) = &self.msg {
            match MSG_TTL.checked_sub(at.elapsed()) {
                Some(left) => ctx.request_repaint_after(left),
                None => {
                    self.msg = None;
                    ctx.request_repaint();
                }
            }
        }
    }
}

/// A text field's rect: one line of `font` tall, centred vertically in `row`.
fn field_rect(ui: &egui::Ui, row: Rect, left: f32, right: f32, font: &FontId) -> Rect {
    let h = ui.fonts_mut(|f| f.row_height(font));
    let cy = row.center().y;
    Rect::from_min_max(pos2(left, cy - h / 2.0), pos2(right, cy + h / 2.0))
}

/// Enter saves, Esc cancels, clicking away saves.
fn list_edit_keys(ui: &egui::Ui, r: &egui::Response, actions: &mut Vec<Action>) {
    if r.lost_focus() {
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            actions.push(Action::CancelListEdit);
        } else {
            actions.push(Action::CommitListEdit);
        }
    }
}

fn list_menu(ui: &mut egui::Ui, l: usize, actions: &mut Vec<Action>) {
    if ui.button("Rename list").clicked() {
        actions.push(Action::StartRename(l));
    }
    if ui.button("Clear done items").clicked() {
        actions.push(Action::ClearDone(l));
    }
    if ui.button("Delete list…").clicked() {
        actions.push(Action::AskDeleteList(l));
    }
}

/// Item text as one line: `#tags` and `due:` dates coloured, done items
/// dimmed and struck through, cut with `…` to fit.
fn item_job(text: &str, done: bool, width: f32, t: &Theme) -> LayoutJob {
    let mut job = LayoutJob::default();
    let font = FontId::proportional(BODY);
    for span in tick_core::doc::spans(text) {
        let (s, color) = match span {
            Span::Plain(s) => (s, t.fg),
            Span::Tag(s) => (s, t.cyan),
            Span::Due(s) => (s, t.yellow),
        };
        let color = if done { t.dim } else { color };
        let mut fmt = TextFormat::simple(font.clone(), c(color));
        if done {
            fmt.strikethrough = Stroke::new(1.0, c(t.muted));
        }
        job.append(s, 0.0, fmt);
    }
    job.wrap = TextWrapping { max_width: width, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    job
}

fn one_line(ui: &egui::Ui, text: &str, font: FontId, color: Color32, width: f32) -> Arc<egui::Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap = TextWrapping { max_width: width, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    ui.fonts_mut(|f| f.layout_job(job))
}

enum Icon {
    Sidebar,
    Help,
}

/// Small square footer button with a painted icon (no icon font needed).
fn icon_button(ui: &mut egui::Ui, t: &Theme, icon: Icon) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(26.0, 22.0), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let col = c(if resp.hovered() { t.fg } else { t.dim });
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, 0, c(t.raised));
    }
    match icon {
        Icon::Sidebar => {
            let r = Rect::from_center_size(rect.center(), vec2(14.0, 11.0));
            p.rect_stroke(r, 0, Stroke::new(1.3, col), StrokeKind::Inside);
            p.rect_filled(Rect::from_min_size(r.min, vec2(5.0, r.height())), 0, col);
        }
        Icon::Help => {
            p.text(rect.center(), Align2::CENTER_CENTER, "?", FontId::new(13.0, bold()), col);
        }
    }
    resp
}

