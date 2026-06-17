use std::env;
use std::path::{Path, PathBuf};

pub struct ProjectConfig {
    pub files: Vec<PathBuf>,
    pub project_name: Option<String>,
    pub env_files: Vec<PathBuf>,
}

impl ProjectConfig {
    pub fn new(files: Vec<PathBuf>, project_name: Option<String>, env_files: Vec<PathBuf>) -> Self {
        Self {
            files,
            project_name,
            env_files,
        }
    }

    pub fn resolve_project_name(&self, project_dir: &Path) -> String {
        if let Some(name) = &self.project_name {
            return name.clone();
        }
        if let Ok(name) = env::var("COMPOSE_PROJECT_NAME") {
            return name;
        }
        project_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("default")
            .to_string()
    }

    pub fn resolve_compose_files(&self) -> Vec<PathBuf> {
        if !self.files.is_empty() {
            return self.files.clone();
        }

        if let Ok(files_env) = env::var("COMPOSE_FILE") {
            let sep = if cfg!(windows) { ";" } else { ":" };
            return files_env.split(sep).map(PathBuf::from).collect();
        }

        let candidates = [
            "compose.yaml",
            "compose.yml",
            "docker-compose.yaml",
            "docker-compose.yml",
        ];
        for c in candidates {
            let path = PathBuf::from(c);
            if path.exists() {
                return vec![path];
            }
        }

        vec![]
    }
}
