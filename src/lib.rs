//! Logic for discovering and searching projects within the Code Control Plane.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProjectStack {
    Rust,
    Go,
    NodeJS,
    Python,
    Monorepo,
    Generic,
}

impl std::fmt::Display for ProjectStack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rust => write!(f, "Rust"),
            Self::Go => write!(f, "Go"),
            Self::NodeJS => write!(f, "NodeJS"),
            Self::Python => write!(f, "Python"),
            Self::Monorepo => write!(f, "Monorepo"),
            Self::Generic => write!(f, "Generic"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectDetail {
    pub name: String,
    pub path: PathBuf,
    pub stack: ProjectStack,
    pub essence: Option<String>,
    pub tokens: Vec<String>,
}

/// Extracts the "essence" of a project from its README.md.
/// Grabs the first 2-3 meaningful sentences.
pub fn extract_essence(project_path: &Path) -> Option<String> {
    let readme_names = ["README.md", "readme.md", "README.markdown"];
    for name in readme_names {
        let path = project_path.join(name);
        if let Ok(content) = fs::read_to_string(&path) {
            // Filter out headers, empty lines, and grab first ~200 chars or first few sentences
            let lines: Vec<&str> = content
                .lines()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .take(3)
                .collect();

            if !lines.is_empty() {
                let combined = lines.join(" ");
                // Limit to 200 chars for the manifest
                if combined.len() > 200 {
                    return Some(format!("{}...", &combined[..197]));
                }
                return Some(combined);
            }
        }
    }
    None
}

/// Finds projects in the given root directory that match the query string (case-insensitive).
pub fn find_projects(root: &Path, query: &str, limit: usize) -> Result<Vec<String>> {
    let mut matches = Vec::new();
    let query_lower = query.to_lowercase();

    let entries = fs::read_dir(root).context(format!("Failed to read directory: {:?}", root))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with('.') {
                continue;
            }

            if name.to_lowercase().contains(&query_lower) {
                matches.push(name.to_string());
            }
        }
    }

    matches.sort();
    if matches.len() > limit {
        matches.truncate(limit);
    }

    Ok(matches)
}

/// Scans a directory to determine its primary tech stack.
pub fn detect_stack(path: &Path) -> ProjectStack {
    let files = fs::read_dir(path)
        .ok()
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();

    // 1. Monorepo Heuristics
    if files.contains(&"nx.json".to_string())
        || files.contains(&"turbo.json".to_string())
        || files.contains(&"go.work".to_string())
        || files.contains(&"lerna.json".to_string())
    {
        return ProjectStack::Monorepo;
    }

    // 2. Language Specifics
    if files.contains(&"Cargo.toml".to_string()) {
        return ProjectStack::Rust;
    }
    if files.contains(&"go.mod".to_string()) {
        return ProjectStack::Go;
    }
    if files.contains(&"package.json".to_string()) {
        return ProjectStack::NodeJS;
    }
    if files.contains(&"requirements.txt".to_string())
        || files.contains(&"pyproject.toml".to_string())
    {
        return ProjectStack::Python;
    }

    ProjectStack::Generic
}

/// Scans the entire root directory for detailed project metadata.
pub fn scan_all_projects(root: &Path) -> Result<Vec<ProjectDetail>> {
    let mut details = Vec::new();

    if !root.exists() {
        return Ok(details);
    }

    let entries = fs::read_dir(root).context(format!("Failed to read directory: {:?}", root))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with('.') {
                continue;
            }

            let stack = detect_stack(&path);
            let essence = extract_essence(&path);

            details.push(ProjectDetail {
                name: name.to_string(),
                path: path.clone(),
                stack,
                essence,
                tokens: Vec::new(),
            });
        }
    }

    details.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(details)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_find_projects() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        fs::create_dir(root.join("Primatif_Core")).unwrap();
        fs::create_dir(root.join("Primatif_UI")).unwrap();
        fs::create_dir(root.join("Other_Project")).unwrap();
        fs::create_dir(root.join(".hidden")).unwrap();

        let results = find_projects(root, "primatif", 10).unwrap();
        assert_eq!(results, vec!["Primatif_Core", "Primatif_UI"]);
    }

    #[test]
    fn test_scan_all_projects() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        // Create a Rust project
        let rust_path = root.join("rust_p");
        fs::create_dir(&rust_path).unwrap();
        fs::write(rust_path.join("Cargo.toml"), "").unwrap();
        fs::write(
            rust_path.join("README.md"),
            "# Rust Project\nThis is a rust project.",
        )
        .unwrap();

        // Create a Go project
        let go_path = root.join("go_p");
        fs::create_dir(&go_path).unwrap();
        fs::write(go_path.join("go.mod"), "").unwrap();

        let projects = scan_all_projects(root).unwrap();
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].name, "go_p");
        assert_eq!(projects[0].stack, ProjectStack::Go);
        assert_eq!(projects[1].name, "rust_p");
        assert_eq!(projects[1].stack, ProjectStack::Rust);
        assert!(projects[1].essence.is_some());
        assert!(
            projects[1]
                .essence
                .as_ref()
                .unwrap()
                .contains("This is a rust project")
        );
    }
}
