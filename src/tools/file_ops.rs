use std::path::{Path, PathBuf};

/// Resolve a path relative to the workspace, preventing path traversal.
///
/// Returns `None` if the resolved path escapes the workspace directory.
pub fn resolve_safe_path(workspace: &Path, requested: &str) -> Option<PathBuf> {
    let requested = requested.trim();
    if requested.is_empty() {
        return None;
    }

    // Canonicalize the workspace for comparison.
    let ws_canonical = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());

    let candidate = if Path::new(requested).is_absolute() {
        PathBuf::from(requested)
    } else {
        ws_canonical.join(requested)
    };

    // For existing paths, canonicalize to resolve symlinks and ..
    // For non-existing paths, normalize component-by-component.
    let resolved = if candidate.exists() {
        candidate
            .canonicalize()
            .unwrap_or_else(|_| candidate.clone())
    } else {
        // Normalize without filesystem access
        normalize_path(&candidate)
    };

    // Check the resolved path is within the workspace.
    if resolved.starts_with(&ws_canonical) {
        Some(resolved)
    } else {
        None
    }
}

/// Normalize a path by resolving `.` and `..` components without filesystem access.
fn normalize_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                components.pop();
            }
            std::path::Component::CurDir => {}
            c => components.push(c),
        }
    }
    components.iter().collect()
}

/// Read a file's content from the workspace.
pub fn read_file(workspace: &Path, file_path: &str) -> String {
    match resolve_safe_path(workspace, file_path) {
        Some(path) => {
            if !path.exists() {
                return format!("File not found: {file_path}");
            }
            if !path.is_file() {
                return format!("Not a file: {file_path}");
            }
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    // Truncate large files to prevent context overflow
                    const MAX_CHARS: usize = 50_000;
                    if content.len() > MAX_CHARS {
                        let truncated = crate::core::utils::safe_truncate(&content, MAX_CHARS);
                        format!(
                            "{}\n\n[... truncated, showing first {} of {} bytes]",
                            truncated,
                            truncated.len(),
                            content.len()
                        )
                    } else {
                        content
                    }
                }
                Err(e) => format!("Error reading file: {e}"),
            }
        }
        None => format!("Access denied: path escapes workspace: {file_path}"),
    }
}

/// Write content to a file in the workspace.
pub fn write_file(workspace: &Path, file_path: &str, content: &str) -> String {
    match resolve_safe_path(workspace, file_path) {
        Some(path) => {
            // Ensure parent directories exist
            #[allow(clippy::collapsible_if)]
            if let Some(parent) = path.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    return format!("Error creating directories: {e}");
                }
            }
            match std::fs::write(&path, content) {
                Ok(()) => format!(
                    "Written {} bytes to {}",
                    content.len(),
                    path.strip_prefix(workspace).unwrap_or(&path).display()
                ),
                Err(e) => format!("Error writing file: {e}"),
            }
        }
        None => format!("Access denied: path escapes workspace: {file_path}"),
    }
}

/// Edit a file by searching for `old` text and replacing with `new` text.
///
/// If `replace_all` is true, all occurrences are replaced; otherwise only the first.
/// Returns an error message if the file doesn't exist or `old` text is not found.
pub fn edit_file(
    workspace: &Path,
    file_path: &str,
    old: &str,
    new: &str,
    replace_all: bool,
) -> String {
    match resolve_safe_path(workspace, file_path) {
        Some(path) => {
            if !path.exists() {
                return format!("File not found: {file_path}");
            }
            if !path.is_file() {
                return format!("Not a file: {file_path}");
            }
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) => return format!("Error reading file: {e}"),
            };

            if old.is_empty() {
                return "Error: 'old' text cannot be empty.".to_string();
            }

            let count = text.matches(old).count();
            if count == 0 {
                return format!("Error: old text not found in {file_path}");
            }

            let updated = if replace_all {
                text.replace(old, new)
            } else {
                text.replacen(old, new, 1)
            };

            match std::fs::write(&path, &updated) {
                Ok(()) => {
                    let replaced = if replace_all { count } else { 1 };
                    let display = path.strip_prefix(workspace).unwrap_or(&path).display();
                    format!("Updated {display}: {replaced} occurrence(s) replaced")
                }
                Err(e) => format!("Error writing file: {e}"),
            }
        }
        None => format!("Access denied: path escapes workspace: {file_path}"),
    }
}

/// List directory contents in the workspace.
pub fn list_directory(workspace: &Path, dir_path: &str) -> String {
    let target = if dir_path.trim().is_empty() {
        workspace.to_path_buf()
    } else {
        match resolve_safe_path(workspace, dir_path) {
            Some(p) => p,
            None => return format!("Access denied: path escapes workspace: {dir_path}"),
        }
    };

    if !target.exists() {
        return format!("Directory not found: {dir_path}");
    }
    if !target.is_dir() {
        return format!("Not a directory: {dir_path}");
    }

    match std::fs::read_dir(&target) {
        Ok(entries) => {
            let mut items: Vec<String> = entries
                .filter_map(|e| e.ok())
                .map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    let ft = e.file_type().ok();
                    let suffix = if ft.is_some_and(|t| t.is_dir()) {
                        "/"
                    } else {
                        ""
                    };
                    let size = e.metadata().ok().map(|m| m.len()).unwrap_or(0);
                    if suffix.is_empty() {
                        format!("  {name}  ({size} bytes)")
                    } else {
                        format!("  {name}/")
                    }
                })
                .collect();
            items.sort();
            if items.is_empty() {
                "(empty directory)".to_string()
            } else {
                items.join("\n")
            }
        }
        Err(e) => format!("Error listing directory: {e}"),
    }
}

/// Search for text within files in the workspace (recursive grep).
///
/// Returns matching lines with file path, line number, and content.
/// Results are capped at 50 matches to prevent context overflow.
pub fn search_files(workspace: &Path, query: &str, path: &str) -> String {
    if query.trim().is_empty() {
        return "Error: query cannot be empty.".to_string();
    }

    let search_root = if path.trim().is_empty() {
        workspace.to_path_buf()
    } else {
        match resolve_safe_path(workspace, path) {
            Some(p) => p,
            None => return format!("Access denied: path escapes workspace: {path}"),
        }
    };

    if !search_root.exists() {
        return format!("Path not found: {path}");
    }

    let query_lower = query.to_lowercase();
    let mut results = Vec::new();
    const MAX_RESULTS: usize = 50;
    const MAX_DEPTH: usize = 10;

    search_recursive(
        workspace,
        &search_root,
        &query_lower,
        &mut results,
        MAX_RESULTS,
        0,
        MAX_DEPTH,
    );

    if results.is_empty() {
        format!("No matches found for: {query}")
    } else {
        let count = results.len();
        let suffix = if count >= MAX_RESULTS {
            format!("\n\n[... capped at {MAX_RESULTS} results]")
        } else {
            String::new()
        };
        format!(
            "{count} match(es) for \"{query}\":\n{}{suffix}",
            results.join("\n")
        )
    }
}

/// Directories to skip during search.
const SKIP_DIRS: &[&str] = &[
    ".git",
    ".crabclaw",
    "target",
    "node_modules",
    ".agent",
    "__pycache__",
    ".venv",
    "dist",
    "build",
];

fn search_recursive(
    workspace: &Path,
    dir: &Path,
    query: &str,
    results: &mut Vec<String>,
    max: usize,
    depth: usize,
    max_depth: usize,
) {
    if results.len() >= max || depth > max_depth {
        return;
    }

    // If it's a file, search it directly
    if dir.is_file() {
        search_file(workspace, dir, query, results, max);
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        if results.len() >= max {
            return;
        }

        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden and known build directories
        if name.starts_with('.') && path.is_dir() {
            continue;
        }
        if path.is_dir() && SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }

        if path.is_dir() {
            search_recursive(workspace, &path, query, results, max, depth + 1, max_depth);
        } else if path.is_file() {
            search_file(workspace, &path, query, results, max);
        }
    }
}

fn search_file(workspace: &Path, file: &Path, query: &str, results: &mut Vec<String>, max: usize) {
    // Skip likely binary files by extension
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
    const BINARY_EXTS: &[&str] = &[
        "png", "jpg", "jpeg", "gif", "ico", "woff", "woff2", "ttf", "eot", "zip", "tar", "gz",
        "bz2", "xz", "pdf", "exe", "dll", "so", "dylib", "o", "a", "class", "jar", "pyc", "wasm",
    ];
    if BINARY_EXTS.contains(&ext) {
        return;
    }

    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => return, // Skip binary/unreadable files
    };

    let rel_path = file
        .strip_prefix(workspace)
        .unwrap_or(file)
        .display()
        .to_string();

    for (line_num, line) in content.lines().enumerate() {
        if results.len() >= max {
            return;
        }
        if line.to_lowercase().contains(query) {
            let trimmed = line.trim();
            let display = if trimmed.len() > 120 {
                format!("{}...", crate::core::utils::safe_truncate(trimmed, 117))
            } else {
                trimmed.to_string()
            };
            results.push(format!("  {rel_path}:{}: {display}", line_num + 1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn resolve_relative_path() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("test.txt"), "hello").unwrap();

        let resolved = resolve_safe_path(dir.path(), "test.txt");
        assert!(resolved.is_some());
    }

    #[test]
    fn reject_path_traversal() {
        let dir = tempdir().unwrap();
        let result = resolve_safe_path(dir.path(), "../../../etc/passwd");
        assert!(result.is_none());
    }

    #[test]
    fn reject_absolute_outside_workspace() {
        let dir = tempdir().unwrap();
        let result = resolve_safe_path(dir.path(), "/etc/passwd");
        assert!(result.is_none());
    }

    #[test]
    fn allow_absolute_inside_workspace() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("inner.txt");
        std::fs::write(&file, "data").unwrap();

        let result = resolve_safe_path(dir.path(), file.to_str().unwrap());
        assert!(result.is_some());
    }

    #[test]
    fn read_existing_file() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("hello.txt"), "world").unwrap();

        let content = read_file(dir.path(), "hello.txt");
        assert_eq!(content, "world");
    }

    #[test]
    fn read_nonexistent_file() {
        let dir = tempdir().unwrap();
        let content = read_file(dir.path(), "nope.txt");
        assert!(content.contains("not found"));
    }

    #[test]
    fn read_outside_workspace_blocked() {
        let dir = tempdir().unwrap();
        let content = read_file(dir.path(), "../../../etc/passwd");
        assert!(content.contains("Access denied"));
    }

    #[test]
    fn write_new_file() {
        let dir = tempdir().unwrap();
        let result = write_file(dir.path(), "out.txt", "hello world");
        assert!(result.contains("Written"));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("out.txt")).unwrap(),
            "hello world"
        );
    }

    #[test]
    fn write_creates_parent_dirs() {
        let dir = tempdir().unwrap();
        let result = write_file(dir.path(), "sub/dir/file.txt", "nested");
        assert!(result.contains("Written"));
        assert!(dir.path().join("sub/dir/file.txt").exists());
    }

    #[test]
    fn write_outside_workspace_blocked() {
        let dir = tempdir().unwrap();
        let result = write_file(dir.path(), "../escape.txt", "bad");
        assert!(result.contains("Access denied"));
    }

    #[test]
    fn list_workspace_root() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();

        let result = list_directory(dir.path(), "");
        assert!(result.contains("a.txt"));
        assert!(result.contains("sub/"));
    }

    #[test]
    fn list_subdirectory() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("mydir");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("inner.txt"), "").unwrap();

        let result = list_directory(dir.path(), "mydir");
        assert!(result.contains("inner.txt"));
    }

    #[test]
    fn list_nonexistent_directory() {
        let dir = tempdir().unwrap();
        let result = list_directory(dir.path(), "nope");
        assert!(result.contains("not found"));
    }

    #[test]
    fn list_outside_workspace_blocked() {
        let dir = tempdir().unwrap();
        let result = list_directory(dir.path(), "../../");
        assert!(result.contains("Access denied"));
    }

    #[test]
    fn empty_path_rejected() {
        let dir = tempdir().unwrap();
        let result = resolve_safe_path(dir.path(), "");
        assert!(result.is_none());
    }

    #[test]
    fn search_finds_matches() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("foo.rs"), "fn hello_world() {}").unwrap();
        std::fs::write(dir.path().join("bar.rs"), "fn goodbye() {}").unwrap();

        let result = search_files(dir.path(), "hello", "");
        assert!(result.contains("1 match"));
        assert!(result.contains("foo.rs"));
    }

    #[test]
    fn search_no_matches() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("foo.rs"), "fn hello() {}").unwrap();

        let result = search_files(dir.path(), "nonexistent_term_xyz", "");
        assert!(result.contains("No matches"));
    }

    #[test]
    fn search_empty_query_rejected() {
        let dir = tempdir().unwrap();
        let result = search_files(dir.path(), "", "");
        assert!(result.contains("cannot be empty"));
    }

    #[test]
    fn search_in_subdirectory() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("src");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("main.rs"), "fn main() { println!(\"hi\") }").unwrap();
        std::fs::write(dir.path().join("readme.md"), "say hi").unwrap();

        // Search only in src/
        let result = search_files(dir.path(), "hi", "src");
        assert!(result.contains("main.rs"));
        assert!(!result.contains("readme.md"));
    }

    #[test]
    fn search_case_insensitive() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("test.rs"), "HELLO World").unwrap();

        let result = search_files(dir.path(), "hello", "");
        assert!(result.contains("1 match"));
    }

    // ── edit_file tests ──────────────────────────────────────────────

    #[test]
    fn edit_replaces_first_occurrence() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "aaa bbb aaa").unwrap();
        let result = edit_file(dir.path(), "a.txt", "aaa", "ccc", false);
        assert!(result.contains("1 occurrence"));
        let content = std::fs::read_to_string(dir.path().join("a.txt")).unwrap();
        assert_eq!(content, "ccc bbb aaa");
    }

    #[test]
    fn edit_replaces_all_occurrences() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "aaa bbb aaa").unwrap();
        let result = edit_file(dir.path(), "a.txt", "aaa", "ccc", true);
        assert!(result.contains("2 occurrence"));
        let content = std::fs::read_to_string(dir.path().join("a.txt")).unwrap();
        assert_eq!(content, "ccc bbb ccc");
    }

    #[test]
    fn edit_old_text_not_found() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello world").unwrap();
        let result = edit_file(dir.path(), "a.txt", "xyz", "abc", false);
        assert!(result.contains("old text not found"));
    }

    #[test]
    fn edit_file_not_found() {
        let dir = tempdir().unwrap();
        let result = edit_file(dir.path(), "nonexistent.txt", "a", "b", false);
        assert!(result.contains("File not found"));
    }

    #[test]
    fn edit_empty_old_rejected() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello").unwrap();
        let result = edit_file(dir.path(), "a.txt", "", "x", false);
        assert!(result.contains("cannot be empty"));
    }

    #[test]
    fn edit_outside_workspace_blocked() {
        let dir = tempdir().unwrap();
        let result = edit_file(dir.path(), "../escape.txt", "a", "b", false);
        assert!(result.contains("Access denied"));
    }

    #[test]
    fn edit_delete_text() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello cruel world").unwrap();
        let result = edit_file(dir.path(), "a.txt", "cruel ", "", false);
        assert!(result.contains("1 occurrence"));
        let content = std::fs::read_to_string(dir.path().join("a.txt")).unwrap();
        assert_eq!(content, "hello world");
    }

    #[test]
    fn edit_multiline_text() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "line1\nline2\nline3\n").unwrap();
        let result = edit_file(dir.path(), "a.txt", "line2\nline3", "replaced", false);
        assert!(result.contains("1 occurrence"));
        let content = std::fs::read_to_string(dir.path().join("a.txt")).unwrap();
        assert_eq!(content, "line1\nreplaced\n");
    }

    // ── Multi-byte / UTF-8 edge case tests ──────────────────────────

    #[test]
    fn search_truncates_long_chinese_line_safely() {
        let dir = tempdir().unwrap();
        // Create a line with >120 bytes of Chinese text (each char = 3 bytes)
        // 50 chars × 3 = 150 bytes > 120
        let long_line = "你".repeat(50); // 150 bytes
        std::fs::write(dir.path().join("cn.txt"), &long_line).unwrap();

        // Should not panic when truncating at byte 117
        let result = search_files(dir.path(), "你", "");
        assert!(result.contains("cn.txt"));
        assert!(result.contains("...")); // truncated
    }

    #[test]
    fn edit_chinese_content() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("cn.txt"), "你好世界").unwrap();
        let result = edit_file(dir.path(), "cn.txt", "世界", "CrabClaw", false);
        assert!(result.contains("1 occurrence"));
        let content = std::fs::read_to_string(dir.path().join("cn.txt")).unwrap();
        assert_eq!(content, "你好CrabClaw");
    }

    #[test]
    fn edit_emoji_content() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("em.txt"), "🦀 is cool 🦀").unwrap();
        let result = edit_file(dir.path(), "em.txt", "🦀 is cool", "🦞 is better", false);
        assert!(result.contains("1 occurrence"));
        let content = std::fs::read_to_string(dir.path().join("em.txt")).unwrap();
        assert_eq!(content, "🦞 is better 🦀");
    }
}
