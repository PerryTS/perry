use async_trait::async_trait;
use perry_container_compose::backend::{ContainerBackend, SecurityProfile};
use perry_container_compose::error::{ComposeError, Result};
use perry_container_compose::types::{
    ComposeNetwork, ComposeServiceBuild, ComposeVolume, ContainerHandle, ContainerInfo,
    ContainerLogs, ContainerSpec, ImageInfo,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct MockBackendState {
    pub containers: HashMap<String, ContainerInfo>,
    pub networks: Vec<String>,
    pub volumes: Vec<String>,
    pub actions: Vec<String>,
    pub fail_on_run: Option<String>, // Substring to fail on
}

#[derive(Clone, Default)]
pub struct MockBackend {
    pub state: Arc<Mutex<MockBackendState>>,
}

#[async_trait]
impl ContainerBackend for MockBackend {
    fn backend_name(&self) -> &str {
        "mock"
    }

    async fn check_available(&self) -> Result<()> {
        Ok(())
    }

    async fn run(&self, spec: &ContainerSpec) -> Result<ContainerHandle> {
        let mut state = self.state.lock().unwrap();
        let name = spec.name.clone().unwrap_or_else(|| "unnamed".to_string());

        if let Some(fail_name) = &state.fail_on_run {
            if name.contains(fail_name) || spec.image.contains(fail_name) {
                return Err(ComposeError::ServiceStartupFailed {
                    service: name,
                    message: "Mock failure".to_string(),
                });
            }
        }

        state.actions.push(format!("run:{}", name));
        let info = ContainerInfo {
            id: name.clone(),
            name: name.clone(),
            image: spec.image.clone(),
            status: "running".to_string(),
            ports: spec.ports.clone().unwrap_or_default(),
            labels: spec.labels.clone().unwrap_or_default(),
            created: "2025-01-01T00:00:00Z".to_string(),
            ip_address: "127.0.0.1".to_string(),
        };
        state.containers.insert(name.clone(), info);
        Ok(ContainerHandle {
            id: name.clone(),
            name: Some(name),
        })
    }

    async fn create(&self, spec: &ContainerSpec) -> Result<ContainerHandle> {
        let mut state = self.state.lock().unwrap();
        let name = spec.name.clone().unwrap_or_else(|| "unnamed".to_string());
        let info = ContainerInfo {
            id: name.clone(),
            name: name.clone(),
            image: spec.image.clone(),
            status: "created".to_string(),
            ports: spec.ports.clone().unwrap_or_default(),
            labels: spec.labels.clone().unwrap_or_default(),
            created: "2025-01-01T00:00:00Z".to_string(),
            ip_address: "".to_string(),
        };
        state.containers.insert(name.clone(), info);
        Ok(ContainerHandle {
            id: name.clone(),
            name: Some(name),
        })
    }

    async fn start(&self, id: &str) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        if let Some(c) = state.containers.get_mut(id) {
            c.status = "running".to_string();
            Ok(())
        } else {
            Err(ComposeError::NotFound(id.to_string()))
        }
    }

    async fn stop(&self, id: &str, _timeout: Option<u32>) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        state.actions.push(format!("stop:{}", id));
        if let Some(c) = state.containers.get_mut(id) {
            c.status = "stopped".to_string();
            Ok(())
        } else {
            Err(ComposeError::NotFound(id.to_string()))
        }
    }

    async fn remove(&self, id: &str, _force: bool) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        state.actions.push(format!("remove:{}", id));
        state.containers.remove(id);
        Ok(())
    }

    async fn list(&self, _all: bool) -> Result<Vec<ContainerInfo>> {
        let state = self.state.lock().unwrap();
        Ok(state.containers.values().cloned().collect())
    }

    async fn inspect(&self, id: &str) -> Result<ContainerInfo> {
        let state = self.state.lock().unwrap();
        state
            .containers
            .get(id)
            .cloned()
            .ok_or_else(|| ComposeError::NotFound(id.to_string()))
    }

    async fn logs(&self, _id: &str, _tail: Option<u32>) -> Result<ContainerLogs> {
        Ok(ContainerLogs {
            stdout: "logs".into(),
            stderr: "".into(),
        })
    }

    async fn wait(&self, _id: &str) -> Result<i32> {
        Ok(0)
    }

    async fn exec(
        &self,
        _id: &str,
        _cmd: &[String],
        _env: Option<&HashMap<String, String>>,
        _workdir: Option<&str>,
    ) -> Result<ContainerLogs> {
        Ok(ContainerLogs {
            stdout: "exec".into(),
            stderr: "".into(),
        })
    }

    async fn build(&self, _spec: &ComposeServiceBuild, _image_name: &str) -> Result<()> {
        Ok(())
    }
    async fn pull_image(&self, _reference: &str) -> Result<()> {
        Ok(())
    }
    async fn list_images(&self) -> Result<Vec<ImageInfo>> {
        Ok(vec![])
    }
    async fn remove_image(&self, _reference: &str, _force: bool) -> Result<()> {
        Ok(())
    }

    async fn create_network(&self, name: &str, _config: &ComposeNetwork) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        state.actions.push(format!("create_network:{}", name));
        state.networks.push(name.to_string());
        Ok(())
    }

    async fn remove_network(&self, name: &str) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        state.actions.push(format!("remove_network:{}", name));
        state.networks.retain(|n| n != name);
        Ok(())
    }

    async fn create_volume(&self, name: &str, _config: &ComposeVolume) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        state.actions.push(format!("create_volume:{}", name));
        state.volumes.push(name.to_string());
        Ok(())
    }

    async fn remove_volume(&self, name: &str) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        state.actions.push(format!("remove_volume:{}", name));
        state.volumes.retain(|v| v != name);
        Ok(())
    }

    async fn inspect_image(&self, _reference: &str) -> Result<ImageInfo> {
        Ok(ImageInfo {
            id: "sha256:mock".into(),
            repository: "mock".into(),
            tag: "latest".into(),
            size: 0,
            created: "".into(),
        })
    }

    async fn run_with_security(
        &self,
        spec: &ContainerSpec,
        _profile: &SecurityProfile,
    ) -> Result<ContainerHandle> {
        self.run(spec).await
    }

    async fn create_with_security(
        &self,
        spec: &ContainerSpec,
        _profile: &SecurityProfile,
    ) -> Result<ContainerHandle> {
        self.create(spec).await
    }

    async fn inspect_network(&self, _name: &str) -> Result<()> {
        let state = self.state.lock().unwrap();
        if state.networks.contains(&_name.to_string()) {
            Ok(())
        } else {
            Err(ComposeError::NotFound(_name.to_string()))
        }
    }

    async fn inspect_volume(&self, _name: &str) -> Result<()> {
        let state = self.state.lock().unwrap();
        if state.volumes.contains(&_name.to_string()) {
            Ok(())
        } else {
            Err(ComposeError::NotFound(_name.to_string()))
        }
    }
}
