//! Command trait and implementations.

use crate::error::Result;
use crate::backend::ContainerBackend;
use async_trait::async_trait;

pub mod build;
pub mod run;
pub mod start;
pub mod stop;
pub mod inspect;

#[async_trait]
pub trait ContainerCommand: Send + Sync {
    async fn exec(&self, backend: &dyn ContainerBackend) -> Result<()>;
}
