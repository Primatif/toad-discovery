use anyhow::{Context, Result};
use rayon::prelude::*;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use toad_core::{
    ActivityTier, ContextType, ProjectDetail, ProjectStatus, SearchResult, StatusReport,
    SubmoduleDetail, TagRegistry, VcsStatus, Workspace, strategy::StrategyRegistry,
};

/// Searches for projects based on a name query and optional tag filter.
pub fn search_projects(
    workspace: &Workspace,
    query: &str,
    tag: Option<&str>,
) -> Result<SearchResult> {
    let registry = toad_core::ProjectRegistry::load(workspace.active_context.as_deref(), None)
        .unwrap_or_default();
    let current_fp = workspace.get_fingerprint().unwrap_or(0);

    let projects = if registry.fingerprint == current_fp && !registry.projects.is_empty() {
        registry.projects
    } else {
        let p = scan_all_projects(workspace)?;
        let new_registry = toad_core::ProjectRegistry {
            fingerprint: current_fp,
            projects: p.clone(),
            last_sync: std::time::SystemTime::now(),
        };
        let _ = new_registry.save(workspace.active_context.as_deref(), None);
        p
    };

    let matches: Vec<_> = projects
        .into_iter()
        .filter(|p| {
            let name_match = p.name.to_lowercase().contains(&query.to_lowercase());
            let tag_match = match tag {
                Some(t) => {
                    let target = if t.starts_with('#') {
                        t.to_string()
                    } else {
                        format!("#{}", t)
                    };
                    p.tags.contains(&target)
                }
                None => true,
            };
            name_match && tag_match
        })
        .collect();

    Ok(SearchResult {
        query: query.to_string(),
        matches,
    })
}

/// Generates a structured status report for the ecosystem.
pub fn generate_status_report(workspace: &Workspace) -> Result<StatusReport> {
    let projects = scan_all_projects(workspace)?;
    let mut status_projects = Vec::new();

    for p in &projects {
        let mut issues = Vec::new();
        let mut is_aligned = true;

        // Check for submodule alignment issues
        for sub in &p.submodules {
            if sub.initialized {
                if let (Some(expected), Some(actual)) = (&sub.expected_commit, &sub.actual_commit)
                    && expected != actual
                {
                    is_aligned = false;
                    issues.push(format!(
                        "Submodule '{}' is out of alignment (expected {}..., found {})",
                        sub.name,
                        &expected[..7],
                        &actual[..7]
                    ));
                }
            } else {
                is_aligned = false;
                issues.push(format!("Submodule '{}' is not initialized", sub.name));
            }
        }

        status_projects.push(ProjectStatus {
            name: p.name.clone(),
            stack: p.stack.clone(),
            activity: p.activity.clone(),
            vcs_status: p.vcs_status.clone(),
            is_aligned,
            issues,
        });
    }

    let healthy_count = status_projects
        .iter()
        .filter(|p| p.vcs_status == VcsStatus::Clean && p.is_aligned)
        .count();

    let summary = format!(
        "{:02}/{} projects are HEALTHY & CLEAN",
        healthy_count,
        status_projects.len()
    );

    let context_type = if workspace.root.join(".gitmodules").exists() {
        ContextType::Hub
    } else if workspace.projects_dir.exists() {
        ContextType::Pond
    } else {
        ContextType::Generic
    };

    Ok(StatusReport {
        active_context: workspace.active_context.clone(),
        context_type,
        projects: status_projects,
        summary,
    })
}

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

fn get_project_metadata(
    path: &Path,
    strategy_registry: &StrategyRegistry,
) -> (String, Vec<String>, Vec<String>, Option<String>) {
    // Identify evidence files
    let files: Vec<String> = fs::read_dir(path)
        .ok()
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();

    let mut taxonomy = Vec::new();
    let mut artifact_dirs = Vec::new();
    let mut stack = "Generic".to_string();

    for strategy in &strategy_registry.strategies {
        if strategy.matches(&files) {
            for tag in &strategy.tags {
                if !taxonomy.contains(tag) {
                    taxonomy.push(tag.clone());
                }
            }
            for artifact in &strategy.artifacts {
                if !artifact_dirs.contains(artifact) {
                    artifact_dirs.push(artifact.clone());
                }
            }
            if stack == "Generic" {
                stack = strategy.name.clone();
            }
        }
    }

    let essence = extract_essence(path);
    (stack, taxonomy, artifact_dirs, essence)
}

fn scan_single_project(
    path: PathBuf,
    strategy_registry: &StrategyRegistry,
    tag_registry: &TagRegistry,
    source: toad_core::TargetSource,
) -> Option<ProjectDetail> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    if name.starts_with('.') {
        return None;
    }

    let (stack, taxonomy, artifact_dirs, essence) = get_project_metadata(&path, strategy_registry);

    let activity = detect_activity(&path);
    let vcs_status = detect_vcs_status(&path);

    let files: Vec<String> = fs::read_dir(&path)
        .ok()
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();

    let sub_projects = if stack.contains("Monorepo")
        || files.contains(&"nx.json".to_string())
        || files.contains(&"turbo.json".to_string())
        || files.contains(&"go.work".to_string())
    {
        discover_sub_projects(&path)
    } else {
        Vec::new()
    };

    // Submodule & Orphan Discovery
    let mut submodules = Vec::new();
    if let Ok(info_list) = toad_git::submodule::parse_gitmodules(&path) {
        for info in info_list {
            let sub_abs_path = path.join(&info.path);
            let (sub_stack, sub_taxonomy, _, sub_essence) =
                get_project_metadata(&sub_abs_path, strategy_registry);

            if let Ok((init, status, expected, actual)) =
                toad_git::submodule::check_submodule_status(&path, &info.path)
            {
                submodules.push(SubmoduleDetail {
                    name: info.name,
                    path: info.path,
                    url: info.url,
                    stack: sub_stack,
                    essence: sub_essence,
                    taxonomy: sub_taxonomy,
                    initialized: init,
                    vcs_status: status,
                    expected_commit: expected,
                    actual_commit: actual,
                });
            }
        }
    }

    // Orphan Detection (child dirs with .git but not in submodules)
    if let Ok(entries) = fs::read_dir(&path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                let entry_name = entry.file_name().to_string_lossy().into_owned();
                if entry_name.starts_with('.')
                    || entry_name == "node_modules"
                    || entry_name == "target"
                {
                    continue;
                }

                // If it's a git repo but not already tracked as a submodule
                if p.join(".git").exists() && !submodules.iter().any(|s| p.ends_with(&s.path)) {
                    let (sub_stack, sub_taxonomy, _, sub_essence) =
                        get_project_metadata(&p, strategy_registry);

                    // Register as an "Orphan" submodule
                    submodules.push(SubmoduleDetail {
                        name: entry_name,
                        path: p.strip_prefix(&path).unwrap_or(&p).to_path_buf(),
                        url: "local".to_string(), // Or try to resolve remote
                        stack: sub_stack,
                        essence: sub_essence,
                        taxonomy: sub_taxonomy,
                        initialized: true,
                        vcs_status: detect_vcs_status(&p),
                        expected_commit: None,
                        actual_commit: None,
                    });
                }
            }
        }
    }

    // Merge persistent tags
    let persistent_tags = tag_registry.get_tags(&name);
    let mut all_tags = taxonomy.clone();
    for t in persistent_tags {
        let tag_with_hash = if t.starts_with('#') {
            t.clone()
        } else {
            format!("#{}", t)
        };
        if !all_tags.contains(&tag_with_hash) {
            all_tags.push(tag_with_hash);
        }
    }
    all_tags.sort();

    Some(ProjectDetail {
        name,
        path,
        stack,
        activity,
        vcs_status,
        essence,
        tags: all_tags,
        taxonomy,
        artifact_dirs,
        sub_projects,
        submodules,
        source,
    })
}

/// Scans the entire root directory for detailed project metadata.
pub fn scan_all_projects(workspace: &Workspace) -> Result<Vec<ProjectDetail>> {
    let strategy_registry = StrategyRegistry::load()?;
    let tags_path = workspace.tags_path();
    let tag_registry = TagRegistry::load(&tags_path).unwrap_or_default();

    let mut details = Vec::new();

    // 1. Scan the root itself (Hub awareness)
    let mut hub_submodule_paths: HashSet<PathBuf> = HashSet::new();
    if let Some(hub_detail) = scan_single_project(
        workspace.root.clone(),
        &strategy_registry,
        &tag_registry,
        toad_core::TargetSource::HubRoot,
    ) {
        // If the root has submodules or is a project itself, include it
        if !hub_detail.submodules.is_empty() || hub_detail.stack != "Generic" {
            // Collect absolute paths of Hub submodules so projects_dir scan can skip them
            for sub in &hub_detail.submodules {
                hub_submodule_paths.insert(workspace.root.join(&sub.path));

                // FLATTENING: Add submodule as a peer project for search/ops
                details.push(ProjectDetail {
                    name: sub.name.clone(),
                    path: workspace.root.join(&sub.path),
                    stack: sub.stack.clone(),
                    activity: detect_activity(&workspace.root.join(&sub.path)),
                    vcs_status: sub.vcs_status.clone(),
                    essence: sub.essence.clone(),
                    tags: {
                        let mut t = sub.taxonomy.clone();
                        // Add persistent tags
                        let p_tags = tag_registry.get_tags(&sub.name);
                        for pt in p_tags {
                            let with_hash = if pt.starts_with('#') {
                                pt
                            } else {
                                format!("#{}", pt)
                            };
                            if !t.contains(&with_hash) {
                                t.push(with_hash);
                            }
                        }
                        t.sort();
                        t
                    },
                    taxonomy: sub.taxonomy.clone(),
                    artifact_dirs: Vec::new(), // Submodules don't have artifacts mapped yet
                    sub_projects: Vec::new(),
                    submodules: Vec::new(),
                    source: toad_core::TargetSource::Submodule,
                });
            }
            details.push(hub_detail);
        }
    }

    // 2. Scan projects directory (legacy/standard layout)
    let root = &workspace.projects_dir;
    if root.exists() {
        let mut projects: Vec<ProjectDetail> = fs::read_dir(root)
            .context(format!("Failed to read directory: {:?}", root))?
            .par_bridge()
            .filter_map(|entry_res| {
                let entry = entry_res.ok()?;
                let path = entry.path();
                // Skip entries already covered as Hub submodules
                if hub_submodule_paths.contains(&path) {
                    return None;
                }
                scan_single_project(
                    path,
                    &strategy_registry,
                    &tag_registry,
                    toad_core::TargetSource::PondProject,
                )
            })
            .collect();
        details.append(&mut projects);
    }

    // Dedup by path as a final safety net
    details.sort_by(|a, b| a.name.cmp(&b.name));
    details.dedup_by(|a, b| a.path == b.path);
    Ok(details)
}

#[cfg(test)]
mod tests;
