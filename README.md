# Tick

A dead-simple to-do list for Omarchy. Your to-dos live in one Markdown file
(`~/Documents/todo.md`) that you can also open in Omawrite, sync, or diff in
git. Tick is three small ways to work that file:

- **`tick`**: a terminal list, launched into a floating window. No dependencies,
  about 4 MB of memory.
- **`tick capture`**: a one-line quick-add box. Type, Enter, gone.
- **`tick-gui`**: a native window in the spirit of Omawrite, with a single
  calm column and an optional lists sidebar (Ctrl+B). About 60 MB of memory,
  zero CPU when idle.

All three take their colors from the current Omarchy theme and re-skin live
when you run `omarchy theme set`. No restart needed.

## Install

```bash
bin/setup              # builds and installs to ~/.local
bin/setup uninstall    # removes it again (your todo.md stays)
```

Then add the keybinds and window rules from `extras/hyprland.lua` to your
Hyprland config:

| Keys | Opens |
|------|-------|
| Super+Shift+T | the list, in a floating terminal |
| Super+Alt+T | quick capture |
| Super+Shift+Ctrl+T | the window version |

There is also a `PKGBUILD` in `pkgbuild/` (`cd pkgbuild && makepkg -si`).

## The file

```markdown
# Today
- [ ] Renew car registration due:2026-10-02
- [ ] Email landlord about the sink #house
- [x] Pay electric bill

# Groceries
- [ ] Eggs
```

Each heading is a list, and each `- [ ]` line is a to-do. Other lines, such as notes,
are kept as they are. `#tags` and `due:` dates are highlighted. Tick saves on every
change with an atomic write. If another program changes the file, Tick reloads it.
Use `--file PATH` or `$TICK_FILE` to point Tick at a different file.

## Command line

```
tick                      open the list
tick add [-l LIST] TEXT   add a to-do (default list: Inbox)
tick capture [-l LIST]    quick-capture box
tick ls [LIST] [--all]    print open to-dos
tick count [LIST]         print how many are open (handy for a bar widget)
tick path                 print the file Tick uses
```

## Keys

**Terminal:** `j`/`k` move · `tab`/`h`/`l` switch list (`1`–`9` jump) · `x` tick ·
`a` add (Enter adds another) · `e` edit · `d` delete · `J`/`K` reorder ·
`m` move to another list · `C` clear done · `N`/`R`/`D` new/rename/delete list ·
`u` undo · `?` help · `q` quit

**Window:** `Ctrl+N` add · `↑`/`↓` select · `Space` tick · `Enter` edit (or click
the text) · `Delete` delete · `Alt+↑`/`↓` reorder · `Ctrl+Tab` or `Ctrl+1`–`9`
switch list · `Ctrl+Shift+N` new list · `Ctrl+B` sidebar · `Ctrl+Z` undo ·
`F1` shortcuts · right-click for more

## Layout

```
core/   parse/write todo.md, the file store, theme colors (no deps, unit tested)
tui/    the `tick` binary: terminal UI, quick capture, CLI (no deps)
gui/    the `tick-gui` binary (egui)
```

```bash
cargo test --workspace
```

The window version bundles iA Writer Mono, the font Omawrite uses, under the
SIL Open Font License 1.1 (`gui/fonts/OFL.txt`).

## Later

Due dates with a Today view, and phone access (sync `todo.md` with Syncthing).
