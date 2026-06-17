//! perry_container_run_capability() for ShellBridge integration.

use super::get_global_backend;
use super::types::{ContainerError, ContainerLogs, ContainerSpec};
use super::verification;
use perry_container_compose::backend::SecurityProfile;
use std::collections::HashMap;
use std::sync::Arc;

pub struct CapabilityGrants {
    pub network: bool,
    pub env: Option<HashMap<String, String>>,
}

pub async fn perry_container_run_capability(
    name: &str,
    image: &str,
    cmd: &[&str],
    grants: &CapabilityGrants,
) -> Result<ContainerLogs, ContainerError> {
    let digest = verification::verify_image(image).await.map_err(|e| {
        ContainerError::VerificationFailed {
            image: image.to_string(),
            reason: e,
        }
    })?;

    let spec = ContainerSpec {
        image: format!("{}@{}", image, digest),
        name: Some(format!("perry-cap-{}-{}", name, rand::random::<u32>())),
        ports: Some(vec![]),
        volumes: Some(vec![]),
        network: if grants.network {
            None
        } else {
            Some("none".to_string())
        },
        rm: Some(true),
        env: grants.env.clone(),
        cmd: Some(cmd.iter().map(|s| s.to_string()).collect()),
        entrypoint: None,
        ..Default::default()
    };

    let backend = Arc::clone(get_global_backend().await?);
    let profile = SecurityProfile {
        read_only_root: true,
        seccomp: Some("default".to_string()),
        no_new_privileges: true,
    };
    let handle = backend
        .run_with_security(&spec, &profile)
        .await
        .map_err(|e| ContainerError::BackendError {
            code: -1,
            message: e.to_string(),
        })?;

    backend
        .logs(&handle.id, None)
        .await
        .map_err(|e| ContainerError::BackendError {
            code: -1,
            message: e.to_string(),
        })
}
