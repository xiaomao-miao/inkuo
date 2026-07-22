//! Filesystem traversal and manipulation helpers shared across modules.
//!
//! Today this module owns one helper, `walk_dir_safe`, which encapsulates
//! the symlink-cycle defence + depth-bound logic that the search command
//! needs. It used to live as a private `fn walk_dir(...)` inside
//! `crate::commands`, which made it impossible for `knowledge/scanner.rs`
//! (and any future caller) to reuse the exact same walking rules.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

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
