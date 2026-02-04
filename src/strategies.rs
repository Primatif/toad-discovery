use std::path::Path;
use toad_core::ProjectStack;

pub trait DiscoveryStrategy {
    fn stack(&self) -> ProjectStack;

    fn matches(&self, files: &[String]) -> bool;
}

pub struct RustStrategy;

impl DiscoveryStrategy for RustStrategy {
    fn stack(&self) -> ProjectStack {
        ProjectStack::Rust
    }

    fn matches(&self, files: &[String]) -> bool {
        files.contains(&"Cargo.toml".to_string())
    }
}

pub struct GoStrategy;

impl DiscoveryStrategy for GoStrategy {
    fn stack(&self) -> ProjectStack {
        ProjectStack::Go
    }

    fn matches(&self, files: &[String]) -> bool {
        files.contains(&"go.mod".to_string())
    }
}

pub struct NodeJSStrategy;

impl DiscoveryStrategy for NodeJSStrategy {
    fn stack(&self) -> ProjectStack {
        ProjectStack::NodeJS
    }

    fn matches(&self, files: &[String]) -> bool {
        files.contains(&"package.json".to_string())
    }
}

pub struct PythonStrategy;

impl DiscoveryStrategy for PythonStrategy {
    fn stack(&self) -> ProjectStack {
        ProjectStack::Python
    }

    fn matches(&self, files: &[String]) -> bool {
        files.contains(&"requirements.txt".to_string())
            || files.contains(&"pyproject.toml".to_string())
    }
}

pub struct MonorepoStrategy;

impl DiscoveryStrategy for MonorepoStrategy {
    fn stack(&self) -> ProjectStack {
        ProjectStack::Monorepo
    }

    fn matches(&self, files: &[String]) -> bool {
        files.contains(&"nx.json".to_string())
            || files.contains(&"turbo.json".to_string())
            || files.contains(&"go.work".to_string())
            || files.contains(&"lerna.json".to_string())
    }
}

pub fn get_strategies() -> Vec<Box<dyn DiscoveryStrategy>> {
    vec![
        Box::new(MonorepoStrategy),
        Box::new(RustStrategy),
        Box::new(GoStrategy),
        Box::new(NodeJSStrategy),
        Box::new(PythonStrategy),
    ]
}

pub fn detect_stack(path: &Path) -> ProjectStack {
    let files = std::fs::read_dir(path)
        .ok()
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();

    for strategy in get_strategies() {
        if strategy.matches(&files) {
            return strategy.stack();
        }
    }

    ProjectStack::Generic
}
