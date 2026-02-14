use crate::detection::{
    detect_activity, detect_vcs_status, discover_sub_projects, extract_essence,
};
use rayon::prelude::*;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use toad_core::{
    ProjectDetail, SubmoduleDetail, TagRegistry, ToadError, ToadResult, Workspace,
    strategy::StrategyRegistry,
};

pub fn find_projects(root: &Path, query: &str, limit: usize) -> ToadResult<Vec<String>> {
    let mut matches = Vec::new();
    let query_lower = query.to_lowercase();

    let entries = fs::read_dir(root).map_err(|e| {
        ToadError::Discovery(format!("Failed to read directory: {:?}: {}", root, e))
    })?;

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

pub fn get_project_metadata(
    path: &Path,
    strategy_registry: &StrategyRegistry,
) -> (String, Vec<String>, Vec<String>, Option<String>) {
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

pub fn scan_single_project(
    path: PathBuf,
    strategy_registry: &StrategyRegistry,
    tag_registry: &TagRegistry,
    source: toad_core::TargetSource,
) -> Option<ProjectDetail> {
    if !path.is_dir() {
        return None;
    }
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
                    path: info.path.into(),
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

                if p.join(".git").exists() && !submodules.iter().any(|s| p.ends_with(&s.path)) {
                    let (sub_stack, sub_taxonomy, _, sub_essence) =
                        get_project_metadata(&p, strategy_registry);

                    submodules.push(SubmoduleDetail {
                        name: entry_name,
                        path: p.strip_prefix(&path).unwrap_or(&p).to_path_buf(),
                        url: "local".to_string(),
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

    // Calculate stats
    let artifact_set: std::collections::HashSet<&str> = artifact_dirs.iter().map(|s| s.as_str()).collect();
    let stats = toad_ops::stats::calculate_project_stats(&path, &artifact_set);

    let dna = crate::detection::detect_dna(&path);

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
        total_size: stats.total_bytes,
        bloat_index: stats.bloat_index,
        dna,
    })
}

pub fn scan_all_projects(workspace: &Workspace) -> ToadResult<Vec<ProjectDetail>> {
    let strategy_registry = StrategyRegistry::load()?;
    let tags_path = workspace.tags_path();
    let tag_registry = TagRegistry::load(&tags_path).unwrap_or_default();

    let mut details = Vec::new();

    // In v1.1.0, projects_dir is where we look for projects.
    // If we are in a Hub style setup, we also scan the workspace root itself.
    let root = &workspace.projects_dir;
    let mut hub_submodule_paths: HashSet<PathBuf> = HashSet::new();

    // The codebase root is either the projects_dir itself (if it's a project)
    // or its parent (if projects_dir is a 'projects/' subdirectory of a workspace).
    let codebase_root = if root.file_name().and_then(|n| n.to_str()) == Some("projects") {
        let parent = root.parent().unwrap_or(root);
        if parent.join(".toad-root").exists() || parent.join(".gitmodules").exists() {
            parent
        } else {
            root
        }
    } else {
        root
    };

    if codebase_root.exists() {
                    if let Some(hub_detail) = scan_single_project(
                        codebase_root.to_path_buf(),
                        &strategy_registry,
                        &tag_registry,
                        toad_core::TargetSource::HubRoot,
                    ) && (!hub_detail.submodules.is_empty() || hub_detail.stack != "Generic")
                    {
                        for sub in &hub_detail.submodules {
                            let sub_abs_path = codebase_root.join(&sub.path);
                            hub_submodule_paths.insert(sub_abs_path.clone());
        
                            details.push(ProjectDetail {
                                name: sub.name.clone(),
                                path: sub_abs_path,
                                stack: sub.stack.clone(),
                                activity: detect_activity(&codebase_root.join(&sub.path)),
                                vcs_status: sub.vcs_status.clone(),
                                essence: sub.essence.clone(),
                                tags: {
                                    let mut t = sub.taxonomy.clone();
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
                                artifact_dirs: Vec::new(),
                                sub_projects: Vec::new(),
                                submodules: Vec::new(),
                                                        source: toad_core::TargetSource::Submodule,
                                                        total_size: 0, // Stats for submodules could be calculated, but 0 for now
                                                        bloat_index: 0.0,
                                                        dna: crate::detection::detect_dna(&root.join(&sub.path)),
                                                    });                        }
                        
                        details.push(hub_detail);
                    }
    }

    if root.exists() {
        let mut projects: Vec<ProjectDetail> = fs::read_dir(root)
            .map_err(|e| {
                ToadError::Discovery(format!("Failed to read directory: {:?}: {}", root, e))
            })?
            .par_bridge()
            .filter_map(|entry_res| {
                let entry = entry_res.ok()?;
                let path = entry.path();
                
                // Skip 'projects' folder itself if we are scanning it
                if path.file_name().and_then(|n| n.to_str()) == Some("projects") {
                    return None;
                }

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

    details.sort_by(|a, b| a.name.cmp(&b.name));
    details.dedup_by(|a, b| a.path == b.path);
    Ok(details)
}
