#[cfg(test)]
mod tests {
    use crate::*;
    use tempfile::tempdir;
    use toad_core::{ActivityTier, ProjectStack};

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
    fn test_activity_detection() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("active_proj");
        fs::create_dir(&path).unwrap();
        assert_eq!(detect_activity(&path), ActivityTier::Active);
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
            "# Rust Project
This is a rust project description that should be captured.",
        )
        .unwrap();

        let projects = scan_all_projects(root).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "rust_p");
        assert_eq!(projects[0].stack, ProjectStack::Rust);
        assert_eq!(projects[0].activity, ActivityTier::Active);
        assert!(projects[0].essence.is_some());
        assert!(
            projects[0]
                .essence
                .as_ref()
                .unwrap()
                .contains("This is a rust project description")
        );
    }
}
