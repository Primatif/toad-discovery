use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};
use toad_core::{ActivityTier, VcsStatus};

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
                        && !l.starts_with("#")
                        && !l.starts_with("![")
                        && !l.starts_with("[![")
                        && !l.starts_with("<")
                        && !l.starts_with("[")
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
