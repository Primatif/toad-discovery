use crate::scanner::scan_all_projects;
use toad_core::{
    ChangelogHistory, ChangeType, ContextType, EcosystemChangelog, ProgressReporter,
    ProjectChange, ProjectStatus, SearchResult, StatusReport, ToadResult, VcsStatus, Workspace,
};

pub fn sync_registry(workspace: &Workspace, reporter: &dyn ProgressReporter) -> ToadResult<usize> {
    reporter.set_message("Discovering projects on disk...");
    let fingerprint = workspace.get_fingerprint()?;
    
    // Load old registry for diffing
    let old_registry = toad_core::ProjectRegistry::load(workspace.active_context.as_deref(), None).unwrap_or_default();
    
    let projects = scan_all_projects(workspace)?;

    reporter.set_message("Generating diff...");
    let changes = generate_diff(&old_registry.projects, &projects);
    if !changes.is_empty() {
        let changelog_entry = EcosystemChangelog {
            timestamp: std::time::SystemTime::now(),
            changes,
        };
        save_changelog(workspace, changelog_entry)?;
    }

    reporter.set_message("Saving to registry...");
    let registry = toad_core::ProjectRegistry {
        fingerprint,
        projects,
        last_sync: std::time::SystemTime::now(),
    };
    registry.save(workspace.active_context.as_deref(), None)?;

    let count = registry.projects.len();
    reporter.finish_with_message("SUCCESS: Registry synchronized.");
    Ok(count)
}

fn generate_diff(old: &[toad_core::ProjectDetail], new: &[toad_core::ProjectDetail]) -> Vec<ProjectChange> {
    let mut changes = Vec::new();
    let old_map: std::collections::HashMap<String, &toad_core::ProjectDetail> = old.iter().map(|p| (p.name.clone(), p)).collect();
    let new_map: std::collections::HashMap<String, &toad_core::ProjectDetail> = new.iter().map(|p| (p.name.clone(), p)).collect();

    for (name, p_new) in &new_map {
        if let Some(p_old) = old_map.get(name) {
            let vcs_changed = p_old.vcs_status != p_new.vcs_status;
            let activity_changed = p_old.activity != p_new.activity;

            if vcs_changed || activity_changed {
                changes.push(ProjectChange {
                    name: name.clone(),
                    change_type: ChangeType::Modified,
                    old_vcs: if vcs_changed { Some(p_old.vcs_status.clone()) } else { None },
                    new_vcs: if vcs_changed { Some(p_new.vcs_status.clone()) } else { None },
                    old_activity: if activity_changed { Some(p_old.activity.clone()) } else { None },
                    new_activity: if activity_changed { Some(p_new.activity.clone()) } else { None },
                });
            }
        } else {
            changes.push(ProjectChange {
                name: name.clone(),
                change_type: ChangeType::Added,
                old_vcs: None,
                new_vcs: Some(p_new.vcs_status.clone()),
                old_activity: None,
                new_activity: Some(p_new.activity.clone()),
            });
        }
    }

    for (name, p_old) in &old_map {
        if !new_map.contains_key(name) {
            changes.push(ProjectChange {
                name: name.clone(),
                change_type: ChangeType::Removed,
                old_vcs: Some(p_old.vcs_status.clone()),
                new_vcs: None,
                old_activity: Some(p_old.activity.clone()),
                new_activity: None,
            });
        }
    }

    changes
}

fn save_changelog(workspace: &Workspace, entry: EcosystemChangelog) -> ToadResult<()> {
    let path = workspace.changelog_path();
    
    // Ensure the directory exists
    workspace.ensure_shadows()?;
    
    let mut history = if path.exists() {
        let content = std::fs::read_to_string(&path)?;
        serde_json::from_str::<ChangelogHistory>(&content).unwrap_or_default()
    } else {
        ChangelogHistory::default()
    };

    history.entries.push(entry);
    
    // Limit history to last 50 entries
    if history.entries.len() > 50 {
        history.entries.remove(0);
    }

    let content = serde_json::to_string_pretty(&history)?;
    std::fs::write(path, content)?;
    Ok(())
}

pub fn search_projects(
    workspace: &Workspace,
    query: &str,
    tag: Option<&str>,
) -> ToadResult<SearchResult> {
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
            let query_lower = query.to_lowercase();
            let name_match = p.name.to_lowercase().contains(&query_lower);
            let stack_match = p.stack.to_lowercase().contains(&query_lower);
            let essence_match = p.essence.as_ref().map(|e| e.to_lowercase().contains(&query_lower)).unwrap_or(false);
            
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
            (name_match || stack_match || essence_match) && tag_match
        })
        .collect();

    Ok(SearchResult {
        query: query.to_string(),
        matches,
    })
}

pub fn generate_status_report(workspace: &Workspace) -> ToadResult<StatusReport> {
    let projects = scan_all_projects(workspace)?;
    let mut status_projects = Vec::new();

    for p in &projects {
        let mut issues = Vec::new();
        let mut is_aligned = true;

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

    let context_type = if workspace.projects_dir.join(".gitmodules").exists() {
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
