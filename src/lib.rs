mod strategies;

use anyhow::{Context, Result};
use rayon::prelude::*;
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};
pub use strategies::detect_stack;
use toad_core::{ActivityTier, ProjectDetail, ProjectStack, VcsStatus};

/// Extracts a high-level "Fingerprint" (mtime) of the workspace.
pub fn get_workspace_fingerprint(root: &Path) -> Result<u64> {
    let metadata = fs::metadata(root).context("Failed to stat projects root")?;
    let mtime = metadata
        .modified()?
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_secs();
    Ok(mtime)
}

/// Extracts the "essence" of a project from its README.md.
/// v2: Grabs up to 10 meaningful lines, excluding images, badges, and HTML.
pub fn extract_essence(project_path: &Path) -> Option<String> {
    let readme_names = ["README.md", "readme.md", "README.markdown"];
    for name in readme_names {
        let path = project_path.join(name);
        if let Ok(content) = fs::read_to_string(&path) {
            let lines: Vec<String> = content
                .lines()
                .map(|l| l.trim())
                .filter(|l| {
                    !l.is_empty()
                        && !l.starts_with("#")   // Headers are good but we usually want the text under them
                        && !l.starts_with("![") // No images
                        && !l.starts_with("[![") // No badges
                        && !l.starts_with("<")    // No HTML
                        && !l.starts_with("[") // No link-only lines
                })
                .take(10)
                .map(|l| l.to_string())
                .collect();

            if !lines.is_empty() {
                let combined = lines.join(" ");
                if combined.len() > 600 {
                    return Some(format!("{}...", &combined[..597]));
                }
                return Some(combined);
            }
        }
    }
    None
}

/// Determines the activity tier based on the last modification time of the directory.
pub fn detect_activity(path: &Path) -> ActivityTier {
    let now = SystemTime::now();
    let mtime = fs::metadata(path).and_then(|m| m.modified()).unwrap_or(now);

    let age = now.duration_since(mtime).unwrap_or(Duration::from_secs(0));
    let day = 24 * 60 * 60;

    if age.as_secs() < 7 * day {
        ActivityTier::Active
    } else if age.as_secs() < 30 * day {
        ActivityTier::Cold
    } else {
        ActivityTier::Archive
    }
}

/// Checks the Git status of the project.
pub fn detect_vcs_status(path: &Path) -> VcsStatus {
    match toad_git::status::check_status(path) {
        Ok(status) => match status {
            toad_git::status::GitStatus::Clean => VcsStatus::Clean,
            toad_git::status::GitStatus::Dirty => VcsStatus::Dirty,
            toad_git::status::GitStatus::Untracked => VcsStatus::Untracked,
            toad_git::status::GitStatus::NoRepo => VcsStatus::None,
        },
        Err(_) => VcsStatus::None,
    }
}

/// Generates procedural hashtags based on project files and stack.
pub fn generate_hashtags(path: &Path, stack: &ProjectStack) -> Vec<String> {
    let mut tags = Vec::new();

    match stack {
        ProjectStack::Rust => tags.push("#rust".to_string()),
        ProjectStack::Go => tags.push("#go".to_string()),
        ProjectStack::NodeJS => tags.push("#nodejs".to_string()),
        ProjectStack::Python => tags.push("#python".to_string()),
        ProjectStack::Monorepo => tags.push("#monorepo".to_string()),
        ProjectStack::Generic => {}
    }

    let files = fs::read_dir(path)
        .ok()
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();

    if files.contains(&"Wails.json".to_string()) || files.contains(&"wails.json".to_string()) {
        tags.push("#desktop".to_string());
        tags.push("#wails".to_string());
    }
    if files.contains(&"Dockerfile".to_string()) {
        tags.push("#docker".to_string());
    }
    if files.contains(&"tauri.conf.json".to_string()) {
        tags.push("#tauri".to_string());
        tags.push("#desktop".to_string());
    }

    tags.sort();
    tags.dedup();
    tags
}

/// Attempts to find sub-projects within a monorepo.
pub fn discover_sub_projects(path: &Path) -> Vec<String> {
    let mut subs = Vec::new();

    let sub_dirs = ["packages", "apps", "services", "crates"];

    for sub in sub_dirs {
        let sub_path = path.join(sub);

        if let Ok(entries) = fs::read_dir(sub_path) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str()
                    && entry.path().is_dir()
                    && !name.starts_with('.')
                    && name != "node_modules"
                    && name != "target"
                {
                    subs.push(name.to_string());
                }
            }
        }
    }

    subs.sort();

    subs
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

/// Scans the entire root directory for detailed project metadata.
pub fn scan_all_projects(root: &Path) -> Result<Vec<ProjectDetail>> {
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut details: Vec<ProjectDetail> = fs::read_dir(root)
        .context(format!("Failed to read directory: {:?}", root))?
        .par_bridge()
        .filter_map(|entry_res| {
            let entry = entry_res.ok()?;
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }

            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }

            let stack = detect_stack(&path);
            let essence = extract_essence(&path);
            let hashtags = generate_hashtags(&path, &stack);
            let activity = detect_activity(&path);
            let vcs_status = detect_vcs_status(&path);
            let sub_projects = if stack == ProjectStack::Monorepo {
                discover_sub_projects(&path)
            } else {
                Vec::new()
            };

            Some(ProjectDetail {
                name,
                path,
                stack,
                activity,
                vcs_status,
                essence,
                hashtags,
                sub_projects,
            })
        })
        .collect();

    details.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(details)
}

#[cfg(test)]
mod tests;
