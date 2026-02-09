use crate::scanner::scan_all_projects;
use toad_core::{
    ContextType, ProgressReporter, ProjectStatus, SearchResult, StatusReport, ToadResult,
    VcsStatus, Workspace,
};

pub fn sync_registry(workspace: &Workspace, reporter: &dyn ProgressReporter) -> ToadResult<usize> {
    reporter.set_message("Discovering projects on disk...");
    let fingerprint = workspace.get_fingerprint()?;
    let projects = scan_all_projects(workspace)?;

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
