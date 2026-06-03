use crate::error::Result;
use crate::backend::ContainerBackend;
use crate::commands::ContainerCommand;
use crate::types::ComposeService;
use async_trait::async_trait;

pub struct StartCommand {
    pub service: ComposeService,
    pub service_name: String,
}

#[async_trait]
impl ContainerCommand for StartCommand {
    async fn exec(&self, backend: &dyn ContainerBackend) -> Result<()> {
        self.service.start_command(backend, &self.service_name).await
    }
}
