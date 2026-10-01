//! `tick`: the terminal frontend and command line for Tick.
//! Zero dependencies beyond tick-core: raw mode via `stty`, rendering via
//! ANSI truecolor, colours from the live Omarchy theme.

mod capture;
mod list;
mod term;

use std::path::PathBuf;
use std::process::exit;

use tick_core::{display_path, Item, Store, INBOX};

const USAGE: &str = "\
Tick: a to-do list in a Markdown file.

Usage:
  tick                      open the list (terminal UI)
  tick add [-l LIST] TEXT   add a to-do (default list: Inbox)
  tick capture [-l LIST]    one-line quick-capture box
  tick ls [LIST] [--all]    print open to-dos (--all includes done)
  tick count [LIST]         print how many to-dos are open
  tick path                 print the file Tick uses

Options:
  -f, --file PATH           use PATH instead of $TICK_FILE or ~/Documents/Tick/todo.md
  -h, --help                show this help
  -V, --version             show the version";

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();

    // Global --file, before the subcommand.
    let mut file: Option<PathBuf> = None;
    while let Some(first) = args.first() {
        if first == "-f" || first == "--file" {
            if args.len() < 2 {
                die("--file needs a path");
            }
            file = Some(PathBuf::from(args.remove(1)));
            args.remove(0);
        } else if let Some(p) = first.strip_prefix("--file=") {
            file = Some(PathBuf::from(p));
            args.remove(0);
        } else {
            break;
        }
    }
    let path = file.unwrap_or_else(tick_core::default_path);

    let cmd = if args.is_empty() { String::new() } else { args.remove(0) };
    let code = match cmd.as_str() {
        "" => list::run(open(&path)),
        "add" | "a" => {
            let (list, words) = take_list(args);
            let text = words.join(" ");
            if text.trim().is_empty() {
                die("nothing to add. Usage: tick add [-l LIST] TEXT");
            }
            let name = list.unwrap_or_else(|| INBOX.to_string());
            let mut store = open(&path);
            let shown = match store.edit(|d| {
                let i = d.ensure(&name);
                d.lists[i].add(Item::new(&text));
                d.lists[i].name.clone()
            }) {
                Ok(n) => n,
                Err(e) => die(&format!("couldn't save {}: {e}", display_path(&path))),
            };
            println!("Added to {shown}: {}", tick_core::doc::clean(&text));
            0
        }
        "capture" | "c" => {
            let (list, rest) = take_list(args);
            if !rest.is_empty() {
                die("capture takes no text; use `tick add` for that");
            }
            capture::run(open(&path), list)
        }
        "ls" | "list" => {
            let all = args.iter().any(|a| a == "--all" || a == "-a");
            let name: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(String::as_str).collect();
            let store = open(&path);
            let doc = &store.doc;
            let lists: Vec<usize> = if name.is_empty() {
                (0..doc.lists.len()).collect()
            } else {
                match doc.find(&name.join(" ")) {
                    Some(i) => vec![i],
                    None => die(&format!("no list called {}", name.join(" "))),
                }
            };
            for (n, &i) in lists.iter().enumerate() {
                let l = &doc.lists[i];
                if n > 0 {
                    println!();
                }
                println!("{} ({} open)", l.name, l.open_count());
                for it in l.items().filter(|it| all || !it.done) {
                    println!("  [{}] {}", if it.done { 'x' } else { ' ' }, it.text);
                }
            }
            0
        }
        "count" => {
            let store = open(&path);
            let n = if args.is_empty() {
                store.doc.open_count()
            } else {
                match store.doc.find(&args.join(" ")) {
                    Some(i) => store.doc.lists[i].open_count(),
                    None => die(&format!("no list called {}", args.join(" "))),
                }
            };
            println!("{n}");
            0
        }
        "path" => {
            println!("{}", path.display());
            0
        }
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            0
        }
        "-V" | "--version" => {
            println!("tick {}", env!("CARGO_PKG_VERSION"));
            0
        }
        other => {
            eprintln!("tick: unknown command `{other}`\n\n{USAGE}");
            2
        }
    };
    exit(code);
}

fn open(path: &std::path::Path) -> Store {
    match Store::open(path) {
        Ok(s) => s,
        Err(e) => die(&format!("couldn't open {}: {e}", display_path(path))),
    }
}

/// Pulls `-l NAME` / `--list NAME` / `--list=NAME` out of `args`.
fn take_list(args: Vec<String>) -> (Option<String>, Vec<String>) {
    let mut list = None;
    let mut rest = Vec::new();
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        if a == "-l" || a == "--list" {
            match it.next() {
                Some(v) => list = Some(v),
                None => die("--list needs a name"),
            }
        } else if let Some(v) = a.strip_prefix("--list=") {
            list = Some(v.to_string());
        } else if a == "--" {
            rest.extend(it.by_ref());
        } else {
            rest.push(a);
        }
    }
    (list, rest)
}

fn die(msg: &str) -> ! {
    eprintln!("tick: {msg}");
    exit(1);
}
