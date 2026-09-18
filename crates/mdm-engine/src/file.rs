//! The single pre-allocated `.mdm.part` file every segment writes into.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::error::EngineError;

/// Suffix of an in-progress download.
pub const PART_SUFFIX: &str = ".mdm.part";

/// Held across "pick a free name + rename" in [`PartFile::finish`], process-wide.
static FINISH_LOCK: Mutex<()> = Mutex::new(());

/// An open part file; cheap to share behind an `Arc`.
pub struct PartFile {
    file: File,
    dir: PathBuf,
    filename: String,
    part_path: PathBuf,
}

impl PartFile {
    /// Open or create `<dir>/<filename>.mdm.part`. A new file with a known
    /// size is pre-allocated so a full disk fails here, not mid-download.
    pub fn open(dir: &Path, filename: &str, size: Option<u64>) -> Result<PartFile, EngineError> {
        std::fs::create_dir_all(dir).map_err(EngineError::from_io)?;
        let part_path = dir.join(format!("{filename}{PART_SUFFIX}"));
        let existed = part_path.exists();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&part_path)
            .map_err(EngineError::from_io)?;
        if !existed {
            if let Some(n) = size {
                file.set_len(n).map_err(EngineError::from_io)?;
            }
        }
        Ok(PartFile {
            file,
            dir: dir.to_owned(),
            filename: filename.to_owned(),
            part_path,
        })
    }

    /// Write all of `buf` at `offset` without touching a shared cursor.
    pub fn write_at(&self, offset: u64, buf: &[u8]) -> std::io::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileExt;
            self.file.write_all_at(buf, offset)
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileExt;
            let mut done = 0usize;
            while done < buf.len() {
                let n = self.file.seek_write(&buf[done..], offset + done as u64)?;
                if n == 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::WriteZero,
                        "seek_write wrote 0 bytes",
                    ));
                }
                done += n;
            }
            Ok(())
        }
    }

    /// Flush to disk.
    pub fn sync(&self) -> std::io::Result<()> {
        self.file.sync_all()
    }

    /// Where the bytes are while downloading.
    pub fn part_path(&self) -> &Path {
        &self.part_path
    }

    /// fsync, then rename to the final name (`name (n).ext` when it is
    /// taken). A file that exists — or that another `finish()` in this
    /// process is claiming at the same moment — is never overwritten.
    pub fn finish(self) -> Result<PathBuf, EngineError> {
        self.file.sync_all().map_err(EngineError::from_io)?;
        // Windows will not rename an open file.
        drop(self.file);

        // "Pick a free name" and "rename onto it" are one step for every
        // download in this process: `rename` replaces an existing file, so
        // two same-name downloads finishing together would otherwise pick
        // the same free name and one would overwrite the other. The fsync
        // above stays outside the lock.
        let _finishing = FINISH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let target = free_name(&self.dir, &self.filename);
        std::fs::rename(&self.part_path, &target).map_err(EngineError::from_io)?;
        Ok(target)
    }

    /// Delete the part file (cancel with "delete partial data").
    pub fn remove(self) -> Result<(), EngineError> {
        drop(self.file);
        std::fs::remove_file(&self.part_path).map_err(EngineError::from_io)
    }
}

/// Split `filename` into (stem, extension) on the last dot, unless that dot
/// is the first character (dotfiles have no extension). Shared with the
/// live-part numbering in `download.rs` so both agree on what "the
/// extension" means.
pub(crate) fn split_ext(filename: &str) -> (&str, &str) {
    match filename.rfind('.') {
        Some(i) if i > 0 => (&filename[..i], &filename[i..]),
        _ => (filename, ""),
    }
}

/// `name.ext` → `name (1).ext`, `name (2).ext` … until one does not exist.
fn free_name(dir: &Path, filename: &str) -> PathBuf {
    let first = dir.join(filename);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = split_ext(filename);
    (1u32..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .expect("an unused name exists")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_and_preallocates() {
        let d = tempfile::tempdir().unwrap();
        let f = PartFile::open(d.path(), "a.bin", Some(4096)).unwrap();
        assert!(f.part_path().ends_with("a.bin.mdm.part"));
        assert_eq!(std::fs::metadata(f.part_path()).unwrap().len(), 4096);
    }

    #[test]
    fn creates_missing_dir() {
        let d = tempfile::tempdir().unwrap();
        let sub = d.path().join("x").join("y");
        PartFile::open(&sub, "a.bin", None).unwrap();
        assert!(sub.join("a.bin.mdm.part").exists());
    }

    #[test]
    fn write_at_positions_bytes_and_reopen_keeps_them() {
        let d = tempfile::tempdir().unwrap();
        let f = PartFile::open(d.path(), "a.bin", Some(10)).unwrap();
        f.write_at(7, b"xyz").unwrap();
        f.write_at(0, b"ab").unwrap();
        drop(f);
        let again = PartFile::open(d.path(), "a.bin", Some(10)).unwrap();
        let bytes = std::fs::read(again.part_path()).unwrap();
        assert_eq!(&bytes[0..2], b"ab");
        assert_eq!(&bytes[7..10], b"xyz");
        assert_eq!(bytes.len(), 10, "reopen must not truncate");
    }

    #[test]
    fn finish_renames_and_resolves_clashes() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.tar.gz"), b"old").unwrap();
        let f = PartFile::open(d.path(), "a.tar.gz", Some(3)).unwrap();
        f.write_at(0, b"new").unwrap();
        let out = f.finish().unwrap();
        assert_eq!(out.file_name().unwrap(), "a.tar (1).gz");
        assert_eq!(std::fs::read(&out).unwrap(), b"new");
        assert!(!d.path().join("a.tar.gz.mdm.part").exists());
    }

    #[test]
    fn finish_without_extension() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("README"), b"").unwrap();
        std::fs::write(d.path().join("README (1)"), b"").unwrap();
        let f = PartFile::open(d.path(), "README", None).unwrap();
        assert_eq!(f.finish().unwrap().file_name().unwrap(), "README (2)");
    }

    #[test]
    fn two_same_name_finishes_at_once_never_overwrite_each_other() {
        // Final review 2026-09-19: `finish()` picked a free name, then renamed;
        // two finishing together picked the same name and one rename replaced
        // the other's file.
        for _ in 0..200 {
            let d = tempfile::tempdir().unwrap();
            let parts: Vec<PartFile> = ["p1", "p2"]
                .iter()
                .map(|p| {
                    let mut f = PartFile::open(d.path(), p, Some(1)).unwrap();
                    f.filename = "same.bin".into();
                    f
                })
                .collect();
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
            let threads: Vec<_> = parts
                .into_iter()
                .map(|f| {
                    let b = barrier.clone();
                    std::thread::spawn(move || {
                        b.wait();
                        f.finish().unwrap()
                    })
                })
                .collect();
            let outs: Vec<PathBuf> = threads.into_iter().map(|t| t.join().unwrap()).collect();
            assert_ne!(outs[0], outs[1]);
            let files = std::fs::read_dir(d.path()).unwrap().count();
            assert_eq!(files, 2, "both finished files exist");
        }
    }

    #[test]
    fn remove_deletes_part() {
        let d = tempfile::tempdir().unwrap();
        let f = PartFile::open(d.path(), "a", None).unwrap();
        let p = f.part_path().to_owned();
        f.remove().unwrap();
        assert!(!p.exists());
    }
}
