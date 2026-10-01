//! The to-do document: a Markdown file where each heading is a list and each
//! `- [ ]` / `- [x]` line under it is an item. Everything else (notes, blank
//! lines, nested bullets) is kept verbatim so hand edits survive a round trip.

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub done: bool,
    pub text: String,
    /// Leading whitespace and bullet char, kept so we write back what we read.
    indent: String,
    bullet: char,
    upper_x: bool,
}

impl Item {
    pub fn new(text: &str) -> Item {
        Item { done: false, text: clean(text), indent: String::new(), bullet: '-', upper_x: false }
    }

    fn parse(line: &str) -> Option<Item> {
        let body = line.trim_start();
        let indent = &line[..line.len() - body.len()];
        let mut chars = body.chars();
        let bullet = chars.next()?;
        if !matches!(bullet, '-' | '*' | '+') {
            return None;
        }
        let rest = chars.as_str().strip_prefix(" [")?;
        let mut chars = rest.chars();
        let mark = chars.next()?;
        let done = match mark {
            ' ' => false,
            'x' | 'X' => true,
            _ => return None,
        };
        let rest = chars.as_str().strip_prefix(']')?;
        if !(rest.is_empty() || rest.starts_with(' ')) {
            return None;
        }
        Some(Item { done, text: rest.trim().to_string(), indent: indent.to_string(), bullet, upper_x: mark == 'X' })
    }

    fn write(&self, out: &mut String) {
        out.push_str(&self.indent);
        out.push(self.bullet);
        out.push_str(match (self.done, self.upper_x) {
            (false, _) => " [ ]",
            (true, false) => " [x]",
            (true, true) => " [X]",
        });
        if !self.text.is_empty() {
            out.push(' ');
            out.push_str(&self.text);
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    Item(Item),
    Other(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct List {
    pub name: String,
    level: usize,
    lines: Vec<Line>,
}

impl List {
    pub fn new(name: &str) -> List {
        List { name: clean(name), level: 1, lines: Vec::new() }
    }

    pub fn items(&self) -> impl Iterator<Item = &Item> {
        self.lines.iter().filter_map(|l| match l {
            Line::Item(it) => Some(it),
            Line::Other(_) => None,
        })
    }

    pub fn len(&self) -> usize {
        self.items().count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn open_count(&self) -> usize {
        self.items().filter(|it| !it.done).count()
    }

    pub fn item(&self, i: usize) -> Option<&Item> {
        self.items().nth(i)
    }

    pub fn item_mut(&mut self, i: usize) -> Option<&mut Item> {
        self.lines
            .iter_mut()
            .filter_map(|l| match l {
                Line::Item(it) => Some(it),
                Line::Other(_) => None,
            })
            .nth(i)
    }

    /// Index into `lines` of the i-th item.
    fn line_of(&self, i: usize) -> Option<usize> {
        self.lines
            .iter()
            .enumerate()
            .filter(|(_, l)| matches!(l, Line::Item(_)))
            .nth(i)
            .map(|(n, _)| n)
    }

    /// Adds an item after the last open one, so new to-dos land above the
    /// finished ones. Returns its item index.
    pub fn add(&mut self, item: Item) -> usize {
        let open: Vec<usize> = self
            .items()
            .enumerate()
            .filter(|(_, it)| !it.done)
            .map(|(i, _)| i)
            .collect();
        let at = match open.last() {
            Some(&last) => last + 1,
            None => 0,
        };
        self.insert(at, item);
        at
    }

    /// Inserts so the new item becomes item index `at`.
    pub fn insert(&mut self, at: usize, item: Item) {
        let pos = match self.line_of(at) {
            Some(n) => n,
            None => match self.line_of(self.len().wrapping_sub(1)) {
                Some(n) => n + 1,
                None => {
                    // No items yet: append after any non-blank leading lines.
                    let mut n = self.lines.len();
                    while n > 0 && matches!(&self.lines[n - 1], Line::Other(s) if s.trim().is_empty()) {
                        n -= 1;
                    }
                    n
                }
            },
        };
        self.lines.insert(pos, Line::Item(item));
    }

    pub fn remove(&mut self, i: usize) -> Option<Item> {
        let n = self.line_of(i)?;
        match self.lines.remove(n) {
            Line::Item(it) => Some(it),
            Line::Other(_) => unreachable!(),
        }
    }

    /// Swaps item `i` with its neighbour (`up` = towards the top). Returns the
    /// item's new index.
    pub fn shift(&mut self, i: usize, up: bool) -> usize {
        let j = if up { i.checked_sub(1) } else { Some(i + 1) };
        match (self.line_of(i), j.and_then(|j| self.line_of(j))) {
            (Some(a), Some(b)) => {
                self.lines.swap(a, b);
                j.unwrap()
            }
            _ => i,
        }
    }

    /// Removes finished items. Returns how many were removed.
    pub fn clear_done(&mut self) -> usize {
        let before = self.lines.len();
        self.lines.retain(|l| !matches!(l, Line::Item(it) if it.done));
        before - self.lines.len()
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Doc {
    preamble: Vec<String>,
    pub lists: Vec<List>,
}

impl Doc {
    pub fn parse(text: &str) -> Doc {
        let mut doc = Doc::default();
        for line in text.lines() {
            if let Some((level, name)) = heading(line) {
                doc.lists.push(List { name: name.to_string(), level, lines: Vec::new() });
                continue;
            }
            match doc.lists.last_mut() {
                Some(list) => list.lines.push(match Item::parse(line) {
                    Some(it) => Line::Item(it),
                    None => Line::Other(line.to_string()),
                }),
                None => doc.preamble.push(line.to_string()),
            }
        }
        doc
    }

    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        let preamble = trim_blank_tail(&self.preamble);
        for line in preamble {
            out.push_str(line);
            out.push('\n');
        }
        for (n, list) in self.lists.iter().enumerate() {
            if n > 0 || !preamble.is_empty() {
                out.push('\n');
            }
            out.push_str(&"#".repeat(list.level));
            out.push(' ');
            out.push_str(&list.name);
            out.push('\n');
            let end = list.lines.len()
                - list.lines.iter().rev().take_while(|l| matches!(l, Line::Other(s) if s.trim().is_empty())).count();
            for line in &list.lines[..end] {
                match line {
                    Line::Item(it) => it.write(&mut out),
                    Line::Other(s) => out.push_str(s),
                }
                out.push('\n');
            }
        }
        out
    }

    /// Case-insensitive lookup by list name.
    pub fn find(&self, name: &str) -> Option<usize> {
        let name = name.trim();
        self.lists.iter().position(|l| l.name.eq_ignore_ascii_case(name))
    }

    /// Index of the list called `name`, creating it at the end if missing.
    pub fn ensure(&mut self, name: &str) -> usize {
        match self.find(name) {
            Some(i) => i,
            None => {
                self.lists.push(List::new(name));
                self.lists.len() - 1
            }
        }
    }

    /// Moves item `i` of list `from` to the open end of list `to`. Returns its
    /// new index in `to`.
    pub fn move_item(&mut self, from: usize, i: usize, to: usize) -> Option<usize> {
        if from == to || to >= self.lists.len() {
            return None;
        }
        let item = self.lists.get_mut(from)?.remove(i)?;
        let done = item.done;
        let list = &mut self.lists[to];
        Some(if done {
            let at = list.len();
            list.insert(at, item);
            at
        } else {
            list.add(item)
        })
    }

    pub fn open_count(&self) -> usize {
        self.lists.iter().map(List::open_count).sum()
    }
}

/// Item text and list names are single-line.
pub fn clean(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn heading(line: &str) -> Option<(usize, &str)> {
    let level = line.bytes().take_while(|&b| b == b'#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &line[level..];
    if !rest.starts_with(' ') {
        return None;
    }
    let name = rest.trim();
    (!name.is_empty()).then_some((level, name))
}

fn trim_blank_tail(lines: &[String]) -> &[String] {
    let end = lines.len() - lines.iter().rev().take_while(|s| s.trim().is_empty()).count();
    &lines[..end]
}

/// One styled run of item text, for frontends that colour `#tags` and
/// `due:` dates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Span<'a> {
    Plain(&'a str),
    Tag(&'a str),
    Due(&'a str),
}

pub fn spans(text: &str) -> Vec<Span<'_>> {
    let mut out = Vec::new();
    let mut plain_start = 0;
    let mut idx = 0;
    for word in text.split(' ') {
        let kind = if word.len() > 1 && word.starts_with('#') {
            Some(Span::Tag(word))
        } else if word.len() > 4 && word.starts_with("due:") {
            Some(Span::Due(word))
        } else {
            None
        };
        if let Some(k) = kind {
            if plain_start < idx {
                out.push(Span::Plain(&text[plain_start..idx]));
            }
            out.push(k);
            plain_start = idx + word.len();
        }
        idx += word.len() + 1;
    }
    if plain_start < text.len() {
        out.push(Span::Plain(&text[plain_start..]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# Today
- [ ] Renew car registration due:2026-10-02
- [ ] Email landlord #house
- [x] Pay electric bill

# Groceries
Notes about the store.
- [ ] Eggs
* [X] Coffee beans
";

    #[test]
    fn round_trips_a_normal_file() {
        let doc = Doc::parse(SAMPLE);
        assert_eq!(doc.lists.len(), 2);
        assert_eq!(doc.lists[0].len(), 3);
        assert_eq!(doc.lists[0].open_count(), 2);
        assert!(doc.lists[1].item(1).unwrap().done);
        assert_eq!(doc.to_markdown(), SAMPLE);
    }

    #[test]
    fn keeps_preamble_and_odd_lines() {
        let text = "Intro line\n\n## Work\n  - [ ] indented\n- not a checkbox\n- [ ]\n";
        let doc = Doc::parse(text);
        assert_eq!(doc.lists[0].name, "Work");
        assert_eq!(doc.lists[0].len(), 2);
        assert_eq!(doc.to_markdown(), text);
    }

    #[test]
    fn rejects_non_items() {
        for s in ["-[ ] x", "- [?] x", "- [x]x", "#Heading", "- [ x"] {
            assert!(Item::parse(s).is_none(), "{s}");
        }
        assert!(heading("#Heading").is_none());
        assert!(heading("####### seven").is_none());
    }

    #[test]
    fn normalises_blank_lines_between_lists() {
        let doc = Doc::parse("# A\n- [ ] a\n\n\n\n# B\n\n- [ ] b\n\n");
        assert_eq!(doc.to_markdown(), "# A\n- [ ] a\n\n# B\n\n- [ ] b\n");
    }

    #[test]
    fn add_goes_above_done_items() {
        let mut doc = Doc::parse(SAMPLE);
        let at = doc.lists[0].add(Item::new("New thing"));
        assert_eq!(at, 2);
        let names: Vec<_> = doc.lists[0].items().map(|i| i.text.as_str()).collect();
        assert_eq!(names[2], "New thing");
        assert_eq!(names[3], "Pay electric bill");
    }

    #[test]
    fn add_to_empty_list_and_new_list() {
        let mut doc = Doc::parse("# Inbox\n\n# Later\n");
        doc.lists[0].add(Item::new("first"));
        let i = doc.ensure("someday");
        assert_eq!(i, 2);
        doc.lists[i].add(Item::new("sourdough"));
        assert_eq!(doc.to_markdown(), "# Inbox\n- [ ] first\n\n# Later\n\n# someday\n- [ ] sourdough\n");
        assert_eq!(doc.ensure("INBOX"), 0);
    }

    #[test]
    fn add_after_notes_in_empty_list() {
        let mut doc = Doc::parse("# Inbox\nSome note\n\n");
        doc.lists[0].add(Item::new("a"));
        assert_eq!(doc.to_markdown(), "# Inbox\nSome note\n- [ ] a\n");
    }

    #[test]
    fn shift_remove_and_clear() {
        let mut doc = Doc::parse(SAMPLE);
        let l = &mut doc.lists[0];
        assert_eq!(l.shift(0, true), 0);
        assert_eq!(l.shift(0, false), 1);
        assert_eq!(l.item(0).unwrap().text, "Email landlord #house");
        assert_eq!(l.shift(2, false), 2);
        assert_eq!(l.clear_done(), 1);
        assert_eq!(l.remove(0).unwrap().text, "Email landlord #house");
        assert_eq!(l.len(), 1);
        assert!(l.remove(5).is_none());
    }

    #[test]
    fn move_between_lists() {
        let mut doc = Doc::parse(SAMPLE);
        assert_eq!(doc.move_item(0, 0, 1), Some(1));
        assert_eq!(doc.lists[1].item(1).unwrap().text, "Renew car registration due:2026-10-02");
        assert_eq!(doc.move_item(0, 1, 1), Some(3)); // done item goes to the bottom
        assert_eq!(doc.move_item(0, 0, 0), None);
        assert_eq!(doc.open_count(), 3);
    }

    #[test]
    fn clean_flattens_whitespace() {
        assert_eq!(Item::new("  a \n b\tc ").text, "a b c");
    }

    #[test]
    fn spans_mark_tags_and_due() {
        assert_eq!(
            spans("Email #house landlord due:fri"),
            vec![Span::Plain("Email "), Span::Tag("#house"), Span::Plain(" landlord "), Span::Due("due:fri")]
        );
        assert_eq!(spans("# not a tag"), vec![Span::Plain("# not a tag")]);
    }
}
