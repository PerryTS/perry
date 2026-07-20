//! ComposeWrapper — thin orchestration adapter over `perry_container_compose::ComposeEngine`.

use super::types::{ComposeHandle, ComposeSpec, ContainerError, ContainerInfo, ContainerLogs};
use perry_container_compose::backend::ContainerBackend;
use perry_container_compose::ComposeEngine;
use std::sync::Arc;

pub struct ComposeWrapper {
    engine: Arc<ComposeEngine>,
}

impl ComposeWrapper {
    pub fn new(spec: ComposeSpec, backend: Arc<dyn ContainerBackend>) -> Self {
        let project_name = spec
            .name
            .clone()
            .unwrap_or_else(|| "perry-stack".to_string());

        Self {
            engine: Arc::new(ComposeEngine::new(spec, project_name, backend)),
        }
    }

    pub fn new_from_engine(engine: Arc<ComposeEngine>) -> Self {
        Self { engine }
    }

    pub fn engine(&self) -> &Arc<ComposeEngine> {
        &self.engine
    }

    pub async fn up(&self) -> Result<ComposeHandle, ContainerError> {
        self.engine.clone().up(&[], true, false, false).await
    }

    pub async fn down(&self, volumes: bool, remove_orphans: bool) -> Result<(), ContainerError> {
        self.engine.down(&[], remove_orphans, volumes).await
    }

    pub async fn ps(&self) -> Result<Vec<ContainerInfo>, ContainerError> {
        self.engine.ps().await
    }

    pub async fn logs(
        &self,
        service: Option<&str>,
        tail: Option<u32>,
    ) -> Result<ContainerLogs, ContainerError> {
        let services = service.map(|s| vec![s.to_string()]).unwrap_or_default();
        let logs_map = self.engine.logs(&services, tail).await?;

        let mut stdout = String::new();
        let mut stderr = String::new();

        for (svc, logs) in logs_map {
            stdout.push_str(&format!("[{}] {}\n", svc, logs));
        }

        Ok(ContainerLogs { stdout, stderr })
    }

    pub async fn exec(
        &self,
        service: &str,
        cmd: &[String],
    ) -> Result<ContainerLogs, ContainerError> {
        self.engine.exec(service, cmd, None, None).await
    }

    pub fn config(&self) -> Result<String, ContainerError> {
        self.engine.config()
    }

    pub async fn start(&self, services: &[String]) -> Result<(), ContainerError> {
        self.engine.start(services).await
    }

    pub async fn stop(&self, services: &[String]) -> Result<(), ContainerError> {
        self.engine.stop(services).await
    }

    pub async fn restart(&self, services: &[String]) -> Result<(), ContainerError> {
        self.engine.restart(services).await
    }
}
