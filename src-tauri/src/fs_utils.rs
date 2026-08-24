//! Filesystem traversal and manipulation helpers shared across modules.
//!
//! This module owns safe directory traversal plus same-directory temporary
//! file helpers used to commit documents and snapshot metadata atomically.
//! Keeping both policies here gives every writer and scanner one canonical
//! implementation instead of subtly different local variants.

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Reserve a unique temporary file next to `target`, keeping the original
/// extension so format-sensitive Office writers still select the right
/// encoder. The file is created with `create_new`, so concurrent saves never
/// share a staging path.
pub fn create_sibling_temp_file(target: &Path) -> io::Result<PathBuf> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    if !parent.as_os_str().is_empty() {
        fs::create_dir_all(parent)?;
    }
    target.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "target path has no file name")
    })?;
    let extension = target.extension();

    for _ in 0..32 {
        let sequence = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut temp_name = OsString::from(".inkuo-tmp-");
        temp_name.push(format!(
            "{}-{}",
            std::process::id(),
            sequence
        ));
        if let Some(extension) = extension {
            temp_name.push(".");
            temp_name.push(extension);
        }

        let candidate = parent.join(temp_name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(_) => return Ok(candidate),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not reserve a unique sibling temp file",
    ))
}

/// Commit a fully-written sibling temp file over `target`.
///
/// Contents are synced before the replace, existing target permissions are
/// preserved, and the temporary file is removed if the replace fails.
pub fn commit_sibling_temp_file(temp_path: &Path, target: &Path) -> io::Result<()> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(temp_path)?
        .sync_all()?;
    if let Ok(metadata) = fs::metadata(target) {
        fs::set_permissions(temp_path, metadata.permissions())?;
    }

    if let Err(error) = replace_file(temp_path, target) {
        let _ = fs::remove_file(temp_path);
        return Err(error);
    }

    // Persist the directory entry on Unix where directories are sync-able.
    // Failure here is best-effort: the target has already been replaced and
    // reporting failure would incorrectly invite a duplicate user save.
    #[cfg(unix)]
    if let Some(parent) = target.parent() {
        if let Ok(directory) = File::open(parent) {
            let _ = directory.sync_all();
        }
    }

    Ok(())
}

/// Write bytes through a same-directory temporary file and atomically replace
/// the destination. This prevents a crash or full disk from leaving a
/// truncated document behind.
pub fn atomic_write(target: &Path, content: &[u8]) -> io::Result<()> {
    if let Some(parent) = target.parent().filter(|path| !path.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let temp_path = create_sibling_temp_file(target)?;
    let result = (|| {
        let mut file = OpenOptions::new().write(true).truncate(true).open(&temp_path)?;
        file.write_all(content)?;
        file.sync_all()?;
        drop(file);
        commit_sibling_temp_file(&temp_path, target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

/// Copy a file through a same-directory staging file before publishing it.
/// Unlike `std::fs::copy`, the staging file keeps its writable permissions
/// until the durability sync is complete, which also handles read-only source
/// documents safely.
pub fn atomic_copy(source: &Path, target: &Path) -> io::Result<u64> {
    if let Some(parent) = target.parent().filter(|path| !path.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let temp_path = create_sibling_temp_file(target)?;
    let result = (|| {
        let mut input = File::open(source)?;
        let mut output = OpenOptions::new().write(true).truncate(true).open(&temp_path)?;
        let copied = io::copy(&mut input, &mut output)?;
        output.sync_all()?;
        drop(output);
        commit_sibling_temp_file(&temp_path, target)?;
        Ok(copied)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

#[cfg(not(windows))]
fn replace_file(source: &Path, target: &Path) -> io::Result<()> {
    fs::rename(source, target)
}

#[cfg(windows)]
fn replace_file(source: &Path, target: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;

    #[link(name = "Kernel32")]
    extern "system" {
        fn MoveFileExW(
            existing_file_name: *const u16,
            new_file_name: *const u16,
            flags: u32,
        ) -> i32;
    }

    let source_wide: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let target_wide: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    let result = unsafe {
        MoveFileExW(
            source_wide.as_ptr(),
            target_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Per-entry information handed to the visitor.
#[derive(Debug, Clone)]
pub struct WalkEntry {
    pub path: PathBuf,
    pub is_dir: bool,
}

/// Recursively walk `root`, calling `visit` for every regular entry
/// (directory or non-symlink file). Symlinks are skipped entirely so a
/// loop or an escape via a malicious symlink cannot break out of the
/// walked tree.
///
/// The default maximum depth is `25` — enough for typical user content
/// (deeply-nested project subdirs, but not enough for a malicious very-deep
/// tree that could exhaust the stack via recursion).
pub fn walk_dir_safe<F>(root: &Path, mut visit: F)
where
    F: FnMut(WalkEntry),
{
    walk_dir_safe_with_depth(root, &mut visit, 25);
}

/// Like [`walk_dir_safe`] but with an explicit max depth.
pub fn walk_dir_safe_with_depth<F>(root: &Path, visit: &mut F, max_depth: usize)
where
    F: FnMut(WalkEntry),
{
    fn inner<F: FnMut(WalkEntry)>(
        dir: &Path,
        visit: &mut F,
        visited: &mut HashSet<PathBuf>,
        depth: usize,
        max_depth: usize,
    ) {
        if depth > max_depth {
            return;
        }

        // Canonicalise so symlink cycles within the workspace (e.g. a dir
        // pointing back to an ancestor) get caught the second time we try
        // to descend into them. Failing canonicalise (e.g. dangling link)
        // means the path isn't readable anyway, so we skip it.
        let canonical = match std::fs::canonicalize(dir) {
            Ok(p) => p,
            Err(_) => return,
        };
        if !visited.insert(canonical) {
            return;
        }

        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            // Skip hidden entries by convention — matches the prior
            // behaviour of `commands::walk_dir`. Real callers that need
            // hidden files can prefilter the visitor.
            if name.starts_with('.') {
                continue;
            }

            let file_type = match entry.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            // Skip symlinks — they can create cycles or escape the
            // workspace.
            if file_type.is_symlink() {
                continue;
            }

            let is_dir = file_type.is_dir();
            visit(WalkEntry {
                path: path.clone(),
                is_dir,
            });

            if is_dir {
                inner(&path, visit, visited, depth + 1, max_depth);
            }
        }
    }

    let mut visited = HashSet::new();
    inner(root, visit, &mut visited, 0, max_depth);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet as Hs;

    #[test]
    fn atomic_write_replaces_existing_file_without_leaving_temp_files() {
        let dir = std::env::temp_dir().join(format!(
            "inkuo_atomic_write_{}_{}",
            std::process::id(),
            TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("report.docx");
        std::fs::write(&target, b"old").unwrap();

        atomic_write(&target, b"new bytes").unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"new bytes");
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("report.docx")]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sibling_temp_file_preserves_extension_for_office_writers() {
        let dir = std::env::temp_dir().join(format!(
            "inkuo_atomic_extension_{}_{}",
            std::process::id(),
            TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let temp = create_sibling_temp_file(&dir.join("book.xlsx")).unwrap();
        assert_eq!(temp.extension().and_then(|value| value.to_str()), Some("xlsx"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_copy_publishes_the_complete_source() {
        let dir = std::env::temp_dir().join(format!(
            "inkuo_atomic_copy_{}_{}",
            std::process::id(),
            TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.docx");
        let target = dir.join("backup.docx.bak");
        std::fs::write(&source, b"complete source bytes").unwrap();

        assert_eq!(atomic_copy(&source, &target).unwrap(), 21);
        assert_eq!(std::fs::read(&target).unwrap(), b"complete source bytes");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn walk_finds_all_non_hidden_entries() {
        let dir = std::env::temp_dir().join("inkuo_walk_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a/b")).unwrap();
        std::fs::write(dir.join("a/x.txt"), "x").unwrap();
        std::fs::write(dir.join("a/.hidden.txt"), "h").unwrap();

        let mut seen = Hs::new();
        walk_dir_safe(&dir, |e| {
            seen.insert(e.path.file_name().unwrap().to_string_lossy().to_string());
        });

        assert!(seen.contains("x.txt"));
        assert!(seen.contains("a"));
        assert!(seen.contains("b"));
        assert!(!seen.contains(".hidden.txt"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn walk_does_not_descend_into_symlink_loop() {
        let dir = std::env::temp_dir().join("inkuo_walk_loop");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();

        // Create a symlink loop: sub/loop -> ../. On canonicalize the
        // walker hits the visited set on the second visit.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("..", dir.join("sub/loop")).unwrap();
        }
        #[cfg(not(unix))]
        {
            // Skip on non-unix platforms — the test relies on symlinks.
            return;
        }

        let count = std::cell::Cell::new(0usize);
        walk_dir_safe(&dir, |_| {
            count.set(count.get() + 1);
        });

        // If symlink defence fails the count would be unbounded (or stack
        // overflow). Assert we visited a finite number of entries.
        assert!(count.get() < 100);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
