use crate::*;
use std::fs;
use std::time::{Duration, SystemTime};
use tempfile::tempdir;
use toad_core::{ActivityTier, VcsStatus, Workspace};

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
fn test_activity_detection() -> Result<()> {
    let dir = tempdir().unwrap();
    let path = dir.path().join("active_proj");
    fs::create_dir(&path).unwrap();
    assert_eq!(detect_activity(&path), ActivityTier::Active);

    // Cold (> 7 days)
    let cold_time = SystemTime::now() - Duration::from_secs(8 * 24 * 60 * 60);
    filetime::set_file_mtime(&path, filetime::FileTime::from_system_time(cold_time))?;
    assert_eq!(detect_activity(&path), ActivityTier::Cold);

    // Archive (> 30 days)
    let archive_time = SystemTime::now() - Duration::from_secs(31 * 24 * 60 * 60);
    filetime::set_file_mtime(&path, filetime::FileTime::from_system_time(archive_time))?;
    assert_eq!(detect_activity(&path), ActivityTier::Archive);
    Ok(())
}

#[test]
fn test_scan_all_projects() -> Result<()> {
    let dir = tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let ws = Workspace::with_root(root.clone());

    // Setup strategies in the temp home
    let config_dir = root.join(".toad");
    unsafe {
        std::env::set_var("TOAD_ROOT", root.to_str().unwrap());
    }
    let builtin_dir = config_dir.join("strategies/builtin");
    fs::create_dir_all(&builtin_dir).unwrap();
    toad_core::strategy::StrategyRegistry::install_defaults(&builtin_dir).unwrap();

    // Create projects dir
    fs::create_dir(root.join("projects")).unwrap();

    // Create a Rust project
    let rust_path = root.join("projects").join("rust_p");
    fs::create_dir(&rust_path).unwrap();
    fs::write(rust_path.join("Cargo.toml"), "").unwrap();
    fs::write(
        rust_path.join("README.md"),
        "# Rust Project\nThis is a rust project description that should be captured.",
    )
    .unwrap();

    let projects = scan_all_projects(&ws).unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].name, "rust_p");
    assert_eq!(projects[0].stack, "Rust");
    assert_eq!(projects[0].activity, ActivityTier::Active);
    assert!(projects[0].taxonomy.contains(&"#rust".to_string()));
    assert!(projects[0].essence.is_some());
    Ok(())
}

#[test]
fn test_detect_vcs_status() -> Result<()> {
    let dir = tempdir()?;
    let p = dir.path();

    // No repo
    assert_eq!(detect_vcs_status(p), VcsStatus::None);

    // Init repo
    std::process::Command::new("git")
        .arg("init")
        .current_dir(p)
        .output()?;
    assert_eq!(detect_vcs_status(p), VcsStatus::Clean);

    // Dirty
    fs::write(p.join("new.txt"), "hi")?;
    assert_eq!(detect_vcs_status(p), VcsStatus::Untracked);
    Ok(())
}

#[test]
fn test_extract_essence_truncation() -> Result<()> {
    let dir = tempdir()?;
    let p = dir.path();
    let long_readme = "Long line. ".repeat(100);
    fs::write(p.join("README.md"), long_readme)?;

    let essence = extract_essence(p);
    assert!(essence.is_some());
    assert!(essence.unwrap().len() <= 600);
    Ok(())
}

#[test]
fn test_discover_sub_projects() -> Result<()> {
    let dir = tempdir()?;
    let p = dir.path();
    let crates_dir = p.join("crates");
    fs::create_dir(&crates_dir)?;
    fs::create_dir(crates_dir.join("sub-a"))?;
    fs::create_dir(crates_dir.join("sub-b"))?;

    let subs = discover_sub_projects(p);
    assert_eq!(subs, vec!["sub-a", "sub-b"]);
    Ok(())
}

#[test]
fn test_scan_all_projects_high_volume() -> Result<()> {
    let dir = tempdir()?;
    let root = dir.path().to_path_buf();
    let ws = Workspace::with_root(root.clone());

    // Setup strategies in the temp home
    let config_dir = root.join(".toad");
    unsafe {
        std::env::set_var("TOAD_ROOT", root.to_str().unwrap());
    }
    let builtin_dir = config_dir.join("strategies/builtin");
    fs::create_dir_all(&builtin_dir).unwrap();
    toad_core::strategy::StrategyRegistry::install_defaults(&builtin_dir).unwrap();

    fs::create_dir(root.join("projects"))?;

    for i in 0..50 {
        let proj_path = root.join("projects").join(format!("proj-{}", i));
        fs::create_dir(&proj_path)?;
        fs::write(proj_path.join("Cargo.toml"), "")?;
    }

    let projects = scan_all_projects(&ws)?;
    assert_eq!(projects.len(), 50);
    // Should be sorted
    assert_eq!(projects[0].name, "proj-0");
    assert_eq!(projects[49].name, "proj-9"); // alphabetical: 0, 1, 10... 49, 5, 6, 7, 8, 9
    Ok(())
}
