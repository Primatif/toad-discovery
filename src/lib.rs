use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// Finds projects in the given root directory that match the query string (case-insensitive).
/// Returns up to `limit` matches, sorted alphabetically.
pub fn find_projects(root: &Path, query: &str, limit: usize) -> Result<Vec<String>> {
    let mut matches = Vec::new();
    let query_lower = query.to_lowercase();

    let entries = fs::read_dir(root)
        .context(format!("Failed to read directory: {:?}", root))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            // Skip hidden directories (like .git, .gemini)
            if name.starts_with('.') {
                continue;
            }

            if name.to_lowercase().contains(&query_lower) {
                matches.push(name.to_string());
            }
        }
    }

    // Sort alphabetically for consistent output
    matches.sort();

    // Truncate to limit
    if matches.len() > limit {
        matches.truncate(limit);
    }

    Ok(matches)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_find_projects() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        // Create fake project directories
        fs::create_dir(root.join("Primatif_Core")).unwrap();
        fs::create_dir(root.join("Primatif_UI")).unwrap();
        fs::create_dir(root.join("Other_Project")).unwrap();
        fs::create_dir(root.join(".hidden")).unwrap(); // Should be ignored

        // Test 1: Case-insensitive match "primatif"
        let results = find_projects(root, "primatif", 10).unwrap();
        assert_eq!(results, vec!["Primatif_Core", "Primatif_UI"]);

        // Test 2: Match "ui"
        let results = find_projects(root, "ui", 10).unwrap();
        assert_eq!(results, vec!["Primatif_UI"]);

        // Test 3: Limit
        let results = find_projects(root, "primatif", 1).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], "Primatif_Core"); // Sorted

        // Test 4: No match
        let results = find_projects(root, "zebra", 10).unwrap();
        assert!(results.is_empty());
    }
}