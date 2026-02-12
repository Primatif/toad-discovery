use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};
use toad_core::{ActivityTier, VcsStatus};

pub fn extract_essence(project_path: &Path) -> Option<String> {
    let readme_names = ["README.md", "readme.md", "README.markdown"];
    for name in readme_names {
        let path = project_path.join(name);
        if let Ok(content) = fs::read_to_string(&path) {
            let mut extracted = Vec::new();
            let mut char_count = 0;
            let limit = 800; // Raised limit for better semantic depth

            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                // Filter out noise (badges, images, html)
                if trimmed.starts_with("![") || trimmed.starts_with("[![") || trimmed.starts_with("<") {
                    continue;
                }

                // Keep headers as context markers
                let is_header = trimmed.starts_with('#');
                
                // Look for capability indicators
                let has_capability = trimmed.to_lowercase().contains("provides") 
                    || trimmed.to_lowercase().contains("exposes")
                    || trimmed.to_lowercase().contains("main entry")
                    || trimmed.to_lowercase().contains("orchestrates");

                if is_header || has_capability || extracted.len() < 15 {
                    let mut to_add = trimmed.to_string();
                    if char_count + to_add.len() > limit {
                        let allowed = if limit > char_count { limit - char_count } else { 0 };
                        if allowed > 3 {
                            to_add.truncate(allowed - 3);
                            to_add.push_str("...");
                        } else {
                            break;
                        }
                    }
                    extracted.push(to_add.clone());
                    char_count += to_add.len() + 1;
                    
                    if char_count >= limit {
                        break;
                    }
                }
            }

            if !extracted.is_empty() {
                let combined = extracted.join(" ");
                return Some(combined);
            }
        }
    }
    None
}

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
