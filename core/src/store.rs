//! Keeps a `Doc` in sync with its file: every edit re-reads the file if
//! something else changed it, applies the change, and writes it back
//! atomically (temp file + rename) so a crash never leaves half a list.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::doc::Doc;

const UNDO_LIMIT: usize = 200;

pub const DEFAULT_FILE: &str = "# Today\n\n# Inbox\n";

/// What we last saw of a file on disk. Cheap to take, and changes whenever
/// the file is rewritten, replaced, or swapped for another one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    modified: Option<std::time::SystemTime>,
    len: u64,
    ino: u64,
}

impl Stamp {
    pub fn of(path: &Path) -> Option<Stamp> {
        use std::os::unix::fs::MetadataExt;
        let m = fs::metadata(path).ok()?;
        Some(Stamp { modified: m.modified().ok(), len: m.len(), ino: m.ino() })
    }
}

pub struct Store {
    path: PathBuf,
    pub doc: Doc,
    stamp: Option<Stamp>,
    undo: Vec<Doc>,
}

impl Store {
    /// Opens the to-do file, creating it (and its folder) with a starter
    /// layout if it does not exist yet.
    pub fn open(path: &Path) -> io::Result<Store> {
        if !path.exists() {
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir)?;
            }
            fs::write(path, DEFAULT_FILE)?;
        }
        // Write through symlinks (e.g. into a Syncthing folder) instead of
        // replacing the link with a regular file.
        let path = fs::canonicalize(path)?;
        let mut store = Store { path, doc: Doc::default(), stamp: None, undo: Vec::new() };
        store.load()?;
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn load(&mut self) -> io::Result<()> {
        let stamp = Stamp::of(&self.path);
        let text = fs::read_to_string(&self.path)?;
        self.doc = Doc::parse(&text);
        self.stamp = stamp;
        Ok(())
    }

    /// True when the file on disk differs from what we last read or wrote.
    pub fn changed_on_disk(&self) -> bool {
        Stamp::of(&self.path) != self.stamp
    }

    /// Reloads if another program changed the file. Returns true if it did.
    /// Undo history is dropped, since it describes the old contents.
    pub fn refresh(&mut self) -> io::Result<bool> {
        if !self.changed_on_disk() {
            return Ok(false);
        }
        self.load()?;
        self.undo.clear();
        Ok(true)
    }

    /// Applies `f` to the latest version of the document and saves it.
    /// Nothing is written (and no undo step recorded) if `f` changed nothing.
    pub fn edit<R>(&mut self, f: impl FnOnce(&mut Doc) -> R) -> io::Result<R> {
        self.refresh()?;
        let before = self.doc.clone();
        let out = f(&mut self.doc);
        if self.doc != before {
            self.save()?;
            self.undo.push(before);
            if self.undo.len() > UNDO_LIMIT {
                self.undo.remove(0);
            }
        }
        Ok(out)
    }

    /// Restores the document as it was before the last edit. Returns false
    /// when there is nothing to undo.
    pub fn undo(&mut self) -> io::Result<bool> {
        if self.refresh()? {
            return Ok(false);
        }
        let Some(prev) = self.undo.pop() else { return Ok(false) };
        self.doc = prev;
        self.save()?;
        Ok(true)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    fn save(&mut self) -> io::Result<()> {
        let dir = self.path.parent().unwrap_or(Path::new("."));
        let name = self.path.file_name().and_then(|n| n.to_str()).unwrap_or("todo.md");
        let tmp = dir.join(format!(".{name}.tick-{}", std::process::id()));
        let result = (|| {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(self.doc.to_markdown().as_bytes())?;
            f.sync_all()?;
            if let Ok(meta) = fs::metadata(&self.path) {
                let _ = fs::set_permissions(&tmp, meta.permissions());
            }
            fs::rename(&tmp, &self.path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result?;
        self.stamp = Stamp::of(&self.path);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Item;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tick-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn creates_file_and_saves_edits() {
        let dir = temp_dir("create");
        let path = dir.join("sub/todo.md");
        let mut s = Store::open(&path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), DEFAULT_FILE);
        s.edit(|d| d.lists[1].add(Item::new("hello"))).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "# Today\n\n# Inbox\n- [ ] hello\n");
        assert!(s.undo().unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), DEFAULT_FILE);
        assert!(!s.undo().unwrap());
        // no temp files left behind
        assert_eq!(fs::read_dir(dir.join("sub")).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn picks_up_external_changes_before_editing() {
        let dir = temp_dir("external");
        let path = dir.join("todo.md");
        fs::write(&path, "# A\n- [ ] one\n").unwrap();
        let mut s = Store::open(&path).unwrap();
        s.edit(|d| d.lists[0].item_mut(0).unwrap().done = true).unwrap();
        // Someone edits the file in Omawrite (different length -> new stamp).
        fs::write(&path, "# A\n- [x] one\n- [ ] two from omawrite\n").unwrap();
        s.edit(|d| d.lists[0].add(Item::new("three"))).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "# A\n- [x] one\n- [ ] two from omawrite\n- [ ] three\n"
        );
        // The external reload cleared undo, so it can't revert their edit.
        assert!(s.undo().unwrap());
        assert!(!s.undo().unwrap());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn writes_through_symlinks() {
        let dir = temp_dir("symlink");
        let real = dir.join("real.md");
        let link = dir.join("todo.md");
        fs::write(&real, "# A\n").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let mut s = Store::open(&link).unwrap();
        s.edit(|d| d.lists[0].add(Item::new("x"))).unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_to_string(&real).unwrap(), "# A\n- [ ] x\n");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn no_op_edits_do_not_write() {
        let dir = temp_dir("noop");
        let path = dir.join("todo.md");
        let mut s = Store::open(&path).unwrap();
        s.edit(|_| ()).unwrap();
        assert!(!s.can_undo());
        fs::remove_dir_all(dir).unwrap();
    }
}
