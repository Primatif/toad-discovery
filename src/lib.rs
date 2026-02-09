pub mod detection;
pub mod reports;
pub mod scanner;

// Re-export everything for backward compatibility and convenience
pub use detection::{detect_activity, detect_vcs_status, discover_sub_projects, extract_essence};
pub use reports::{generate_status_report, search_projects, sync_registry};
pub use scanner::{find_projects, get_project_metadata, scan_all_projects, scan_single_project};

#[cfg(test)]
mod tests;
