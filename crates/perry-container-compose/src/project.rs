use crate::config::ProjectConfig;
use crate::error::{ComposeError, Result};
use crate::types::ComposeSpec;
use crate::yaml;
use std::path::PathBuf;

pub struct ComposeProject {
    pub spec: ComposeSpec,
    pub project_name: String,
    pub project_dir: PathBuf,
    pub compose_files: Vec<PathBuf>,
}

impl ComposeProject {
    pub fn load(config: &ProjectConfig) -> Result<Self> {
        let project_dir = std::env::current_dir().map_err(ComposeError::IoError)?;
        let project_name = config.resolve_project_name(&project_dir);
        let compose_files = config.resolve_compose_files();

        if compose_files.is_empty() {
            return Err(ComposeError::FileNotFound {
                path: "No compose file found (tried compose.yaml, docker-compose.yml, etc.)".into(),
            });
        }

        // Load environment
        let env = yaml::load_env(&project_dir, &config.env_files);

        // Parse and merge files
        let spec = yaml::parse_and_merge_files(&compose_files, &env)?;

        Ok(Self {
            spec,
            project_name,
            project_dir,
            compose_files,
        })
    }
}
