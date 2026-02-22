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
                        format!(
                            "{}\n\n[... truncated, showing first {} of {} chars]",
                            &content[..MAX_CHARS],
                            MAX_CHARS,
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
}
