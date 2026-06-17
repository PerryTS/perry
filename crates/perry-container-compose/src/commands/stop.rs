use crate::error::Result;
use crate::backend::ContainerBackend;
use crate::commands::ContainerCommand;
use crate::types::ComposeService;
use crate::service::service_container_name;
use async_trait::async_trait;

pub struct StopCommand {
    pub service: ComposeService,
    pub service_name: String,
}

#[async_trait]
impl ContainerCommand for StopCommand {
    async fn exec(&self, backend: &dyn ContainerBackend) -> Result<()> {
        let name = service_container_name(&self.service, &self.service_name);
        backend.stop(&name, None).await
    }
}
