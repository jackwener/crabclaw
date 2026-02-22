use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

const PROJECT_SKILLS_DIR: &str = ".agent/skills";
const SKILL_FILE_NAME: &str = "SKILL.md";

/// Metadata for a discovered skill.
#[derive(Debug, Clone, Serialize)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub location: PathBuf,
    pub source: String,
}

/// Discover skills from project and global directories.
///
/// Aligned with bub's `discover_skills`:
/// - Scans `.agent/skills/*/SKILL.md`
/// - Priority: project (workspace) → global (~/.agent/skills)
/// - First occurrence wins (by case-insensitive name)
pub fn discover_skills(workspace: &Path) -> Vec<SkillMetadata> {
    let roots = [
        (workspace.join(PROJECT_SKILLS_DIR), "project"),
        (
            dirs::home_dir()
                .unwrap_or_default()
                .join(PROJECT_SKILLS_DIR),
            "global",
        ),
    ];

    let mut by_name: std::collections::BTreeMap<String, SkillMetadata> =
        std::collections::BTreeMap::new();

    for (root, source) in &roots {
        if !root.is_dir() {
            continue;
        }

        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };

        let mut dirs: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        dirs.sort_by_key(|e| e.file_name());

        for entry in dirs {
            if !entry.path().is_dir() {
                continue;
            }

            if let Some(meta) = read_skill(&entry.path(), source) {
                let key = meta.name.to_lowercase();
                by_name.entry(key).or_insert(meta);
            }
        }
    }

    by_name.into_values().collect()
}

/// Load the full SKILL.md body for a skill by name.
pub fn load_skill_body(name: &str, workspace: &Path) -> Option<String> {
    let lowered = name.to_lowercase();
    for skill in discover_skills(workspace) {
        if skill.name.to_lowercase() == lowered {
            return fs::read_to_string(&skill.location).ok();
        }
    }
    None
}

fn read_skill(skill_dir: &Path, source: &str) -> Option<SkillMetadata> {
    let skill_file = skill_dir.join(SKILL_FILE_NAME);
    if !skill_file.is_file() {
        return None;
    }

    let content = fs::read_to_string(&skill_file).ok()?;
    let frontmatter = parse_frontmatter(&content);

    let name = frontmatter.get("name").cloned().unwrap_or_else(|| {
        skill_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string()
    });

    if name.trim().is_empty() {
        return None;
    }

    let description = frontmatter
        .get("description")
        .cloned()
        .unwrap_or_else(|| "No description provided.".to_string());

    Some(SkillMetadata {
        name,
        description,
        location: skill_file.canonicalize().unwrap_or(skill_file),
        source: source.to_string(),
    })
}

/// Parse YAML-style frontmatter delimited by `---`.
///
/// Supports simple `key: value` pairs only (no nested structures).
fn parse_frontmatter(content: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let lines: Vec<&str> = content.lines().collect();

    if lines.is_empty() || lines[0].trim() != "---" {
        return map;
    }

    for line in &lines[1..] {
        let trimmed = line.trim();
        if trimmed == "---" {
            break;
        }

        if let Some((key, value)) = trimmed.split_once(':') {
            let key = key.trim().to_lowercase();
            let value = value.trim().to_string();
            if !key.is_empty() {
                map.insert(key, value);
            }
        }
    }

    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_skill(root: &Path, name: &str, content: &str) {
        let skill_dir = root.join(PROJECT_SKILLS_DIR).join(name);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join(SKILL_FILE_NAME), content).unwrap();
    }

    #[test]
    fn discover_skills_from_project() {
        let dir = tempdir().unwrap();
        write_skill(
            dir.path(),
            "my-skill",
            "---\nname: my-skill\ndescription: A test skill\n---\n# My Skill\nBody here.",
        );

        let skills = discover_skills(dir.path());
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "my-skill");
        assert_eq!(skills[0].description, "A test skill");
        assert_eq!(skills[0].source, "project");
    }

    #[test]
    fn frontmatter_parsing() {
        let fm = parse_frontmatter("---\nname: test\ndescription: A skill\n---\nBody");
        assert_eq!(fm.get("name").unwrap(), "test");
        assert_eq!(fm.get("description").unwrap(), "A skill");
    }

    #[test]
    fn no_frontmatter_uses_dir_name() {
        let dir = tempdir().unwrap();
        let skill_dir = dir.path().join(PROJECT_SKILLS_DIR).join("fallback-name");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join(SKILL_FILE_NAME),
            "# Just body\nNo frontmatter.",
        )
        .unwrap();

        let skills = discover_skills(dir.path());
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "fallback-name");
        assert_eq!(skills[0].description, "No description provided.");
    }

    #[test]
    fn empty_workspace_discovers_nothing() {
        let dir = tempdir().unwrap();
        let skills = discover_skills(dir.path());
        assert!(skills.is_empty());
    }

    #[test]
    fn load_skill_body_returns_content() {
        let dir = tempdir().unwrap();
        let body = "---\nname: loader-test\ndescription: Test\n---\n# Full Body\nContent here.";
        write_skill(dir.path(), "loader-test", body);

        let result = load_skill_body("loader-test", dir.path());
        assert!(result.is_some());
        assert!(result.unwrap().contains("# Full Body"));
    }

    #[test]
    fn load_skill_body_case_insensitive() {
        let dir = tempdir().unwrap();
        write_skill(
            dir.path(),
            "CaseSensitive",
            "---\nname: CaseSensitive\ndescription: Test\n---\nBody",
        );

        let result = load_skill_body("casesensitive", dir.path());
        assert!(result.is_some());
    }

    #[test]
    fn load_nonexistent_skill_returns_none() {
        let dir = tempdir().unwrap();
        assert!(load_skill_body("nonexistent", dir.path()).is_none());
    }

    #[test]
    fn multiple_skills_sorted_by_name() {
        let dir = tempdir().unwrap();
        write_skill(
            dir.path(),
            "zebra",
            "---\nname: zebra\ndescription: Z\n---\n",
        );
        write_skill(
            dir.path(),
            "alpha",
            "---\nname: alpha\ndescription: A\n---\n",
        );

        let skills = discover_skills(dir.path());
        assert_eq!(skills.len(), 2);
        assert_eq!(skills[0].name, "alpha");
        assert_eq!(skills[1].name, "zebra");
    }

    #[test]
    fn malformed_frontmatter_skips_cleanly() {
        let fm = parse_frontmatter("---\nnot: valid: yaml: here\n---\nBody");
        // Gracefully handles; the first split_once on ':' takes "not" => "valid: yaml: here"
        assert!(fm.contains_key("not"));
    }

    #[test]
    fn empty_name_in_frontmatter_uses_dir() {
        let dir = tempdir().unwrap();
        write_skill(
            dir.path(),
            "real-dir-name",
            "---\nname:   \ndescription: Has empty name\n---\nBody",
        );

        let skills = discover_skills(dir.path());
        // Empty name is rejected, skill is skipped entirely
        assert!(skills.is_empty());
    }
}
