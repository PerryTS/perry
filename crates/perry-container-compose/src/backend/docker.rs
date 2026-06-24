use super::*;
use crate::error::{ComposeError, Result};
use crate::types::{
    ComposeNetwork, ComposeServiceBuild, ComposeVolume, ContainerInfo, ContainerSpec, ImageInfo,
};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub(crate) struct DockerListEntry {
    #[serde(rename = "ID", alias = "Id", default)]
    pub(crate) id: String,
    #[serde(rename = "Names", default)]
    pub(crate) names: Vec<String>,
    #[serde(rename = "Image", default)]
    pub(crate) image: String,
    #[serde(rename = "Status", alias = "State", default)]
    pub(crate) status: String,
    #[serde(rename = "Ports", default)]
    pub(crate) ports: Vec<String>,
    #[serde(rename = "Labels", default)]
    pub(crate) labels: serde_json::Value,
    #[serde(rename = "Created", alias = "CreatedAt", default)]
    pub(crate) created: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DockerInspectOutput {
    #[serde(rename = "Id")]
    pub(crate) id: String,
    #[serde(rename = "Name")]
    pub(crate) name: String,
    #[serde(rename = "Config")]
    pub(crate) config: DockerInspectConfig,
    #[serde(rename = "State")]
    pub(crate) state: DockerInspectState,
    #[serde(rename = "Created")]
    pub(crate) created: String,
    #[serde(rename = "NetworkSettings", default)]
    pub(crate) network_settings: Option<DockerInspectNetworkSettings>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DockerInspectConfig {
    #[serde(rename = "Image")]
    pub(crate) image: String,
    #[serde(rename = "Labels", default)]
    pub(crate) labels: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DockerInspectState {
    #[serde(rename = "Status")]
    pub(crate) status: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DockerInspectNetworkSettings {
    #[serde(rename = "IPAddress", default)]
    pub(crate) ip_address: String,
    #[serde(rename = "Networks", default)]
    pub(crate) networks: HashMap<String, DockerInspectNetwork>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DockerInspectNetwork {
    #[serde(rename = "IPAddress", default)]
    pub(crate) ip_address: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DockerImageEntry {
    #[serde(rename = "ID", alias = "Id", default)]
    pub(crate) id: String,
    #[serde(rename = "Repositories", alias = "Repository", default)]
    pub(crate) repository: String,
    #[serde(rename = "Tag", default)]
    pub(crate) tag: String,
    #[serde(rename = "Size", default)]
    pub(crate) size: u64,
    #[serde(rename = "Created", alias = "CreatedAt", default)]
    pub(crate) created: String,
}

pub struct DockerProtocol;

impl CliProtocol for DockerProtocol {
    fn run_args(&self, spec: &ContainerSpec) -> Vec<String> {
        let mut args = vec!["run".into(), "--detach".into()];
        if let Some(name) = &spec.name {
            args.extend(["--name".into(), name.clone()]);
        }
        for port in spec.ports.as_ref().iter().flat_map(|v| v.iter()) {
            args.extend(["-p".into(), port.clone()]);
        }
        for vol in spec.volumes.as_ref().iter().flat_map(|v| v.iter()) {
            args.extend(["-v".into(), vol.clone()]);
        }
        for (k, v) in spec.env.as_ref().iter().flat_map(|m| m.iter()) {
            args.extend(["-e".into(), format!("{k}={v}")]);
        }
        for (k, v) in spec.labels.as_ref().iter().flat_map(|m| m.iter()) {
            args.extend(["--label".into(), format!("{k}={v}")]);
        }
        if let Some(net) = &spec.network {
            args.extend(["--network".into(), net.clone()]);
        }
        // Service-key network alias — registers the service KEY (e.g.
        // `db`, `api`) as a DNS name on the attached network, so
        // sibling containers can resolve `db:5432` directly. This
        // matches docker-compose semantics; pre-fix Perry's compose
        // engine relied on the user setting `container_name`
        // explicitly, which broke any compose stack ported from the
        // wider ecosystem.
        if let Some(aliases) = &spec.network_aliases {
            for alias in aliases {
                args.extend(["--network-alias".into(), alias.clone()]);
            }
        }
        if spec.rm.unwrap_or(false) {
            args.push("--rm".into());
        }
        if spec.read_only.unwrap_or(false) {
            args.push("--read-only".into());
        }
        if spec.privileged.unwrap_or(false) {
            args.push("--privileged".into());
        }
        if let Some(user) = &spec.user {
            args.extend(["--user".into(), user.clone()]);
        }
        if let Some(wd) = &spec.workdir {
            args.extend(["--workdir".into(), wd.clone()]);
        }
        if let Some(caps) = &spec.cap_add {
            for cap in caps {
                args.extend(["--cap-add".into(), cap.clone()]);
            }
        }
        if let Some(caps) = &spec.cap_drop {
            for cap in caps {
                args.extend(["--cap-drop".into(), cap.clone()]);
            }
        }
        if let Some(ep) = &spec.entrypoint {
            args.push("--entrypoint".into());
            args.push(ep.join(" "));
        }
        args.push(spec.image.clone());
        for c in spec.cmd.as_ref().iter().flat_map(|v| v.iter()) {
            args.push(c.clone());
        }
        args
    }

    fn create_args(&self, spec: &ContainerSpec) -> Vec<String> {
        let mut args = vec!["create".into()];
        if let Some(name) = &spec.name {
            args.extend(["--name".into(), name.clone()]);
        }
        for port in spec.ports.as_ref().iter().flat_map(|v| v.iter()) {
            args.extend(["-p".into(), port.clone()]);
        }
        for vol in spec.volumes.as_ref().iter().flat_map(|v| v.iter()) {
            args.extend(["-v".into(), vol.clone()]);
        }
        for (k, v) in spec.env.as_ref().iter().flat_map(|m| m.iter()) {
            args.extend(["-e".into(), format!("{k}={v}")]);
        }
        for (k, v) in spec.labels.as_ref().iter().flat_map(|m| m.iter()) {
            args.extend(["--label".into(), format!("{k}={v}")]);
        }
        if let Some(net) = &spec.network {
            args.extend(["--network".into(), net.clone()]);
        }
        if spec.read_only.unwrap_or(false) {
            args.push("--read-only".into());
        }
        if spec.privileged.unwrap_or(false) {
            args.push("--privileged".into());
        }
        if let Some(user) = &spec.user {
            args.extend(["--user".into(), user.clone()]);
        }
        if let Some(wd) = &spec.workdir {
            args.extend(["--workdir".into(), wd.clone()]);
        }
        if let Some(caps) = &spec.cap_add {
            for cap in caps {
                args.extend(["--cap-add".into(), cap.clone()]);
            }
        }
        if let Some(caps) = &spec.cap_drop {
            for cap in caps {
                args.extend(["--cap-drop".into(), cap.clone()]);
            }
        }
        if let Some(ep) = &spec.entrypoint {
            args.push("--entrypoint".into());
            args.push(ep.join(" "));
        }
        args.push(spec.image.clone());
        for c in spec.cmd.as_ref().iter().flat_map(|v| v.iter()) {
            args.push(c.clone());
        }
        args
    }

    fn start_args(&self, id: &str) -> Vec<String> {
        vec!["start".into(), id.into()]
    }

    fn stop_args(&self, id: &str, timeout: Option<u32>) -> Vec<String> {
        let mut args = vec!["stop".into()];
        if let Some(t) = timeout {
            args.extend(["--time".into(), t.to_string()]);
        }
        args.push(id.into());
        args
    }

    fn remove_args(&self, id: &str, force: bool) -> Vec<String> {
        let mut args = vec!["rm".into()];
        if force {
            args.push("-f".into());
        }
        args.push(id.into());
        args
    }

    fn list_args(&self, all: bool) -> Vec<String> {
        let mut args = vec!["ps".into(), "--format".into(), "json".into()];
        if all {
            args.push("--all".into());
        }
        args
    }

    fn inspect_args(&self, id: &str) -> Vec<String> {
        vec![
            "inspect".into(),
            "--format".into(),
            "json".into(),
            id.into(),
        ]
    }

    fn logs_args(&self, id: &str, tail: Option<u32>) -> Vec<String> {
        let mut args = vec!["logs".into()];
        if let Some(t) = tail {
            args.extend(["--tail".into(), t.to_string()]);
        }
        args.push(id.into());
        args
    }

    fn exec_args(
        &self,
        id: &str,
        cmd: &[String],
        env: Option<&HashMap<String, String>>,
        workdir: Option<&str>,
    ) -> Vec<String> {
        let mut args = vec!["exec".into()];
        if let Some(w) = workdir {
            args.extend(["--workdir".into(), w.into()]);
        }
        if let Some(e) = env {
            for (k, v) in e {
                args.extend(["-e".into(), format!("{k}={v}")]);
            }
        }
        args.push(id.into());
        args.extend(cmd.iter().cloned());
        args
    }

    fn pull_image_args(&self, reference: &str) -> Vec<String> {
        vec!["pull".into(), reference.into()]
    }

    fn list_images_args(&self) -> Vec<String> {
        vec!["images".into(), "--format".into(), "json".into()]
    }

    fn remove_image_args(&self, reference: &str, force: bool) -> Vec<String> {
        let mut args = vec!["rmi".into()];
        if force {
            args.push("-f".into());
        }
        args.push(reference.into());
        args
    }

    fn create_network_args(&self, name: &str, config: &ComposeNetwork) -> Vec<String> {
        let mut args = vec!["network".into(), "create".into()];
        if let Some(d) = &config.driver {
            args.extend(["--driver".into(), d.clone()]);
        }
        if let Some(lbls) = &config.labels {
            for (k, v) in lbls.to_map() {
                args.extend(["--label".into(), format!("{k}={v}")]);
            }
        }
        args.push(name.into());
        args
    }

    fn remove_network_args(&self, name: &str) -> Vec<String> {
        vec!["network".into(), "rm".into(), name.into()]
    }

    fn create_volume_args(&self, name: &str, config: &ComposeVolume) -> Vec<String> {
        let mut args = vec!["volume".into(), "create".into()];
        if let Some(d) = &config.driver {
            args.extend(["--driver".into(), d.clone()]);
        }
        if let Some(lbls) = &config.labels {
            for (k, v) in lbls.to_map() {
                args.extend(["--label".into(), format!("{k}={v}")]);
            }
        }
        args.push(name.into());
        args
    }

    fn remove_volume_args(&self, name: &str) -> Vec<String> {
        vec!["volume".into(), "rm".into(), name.into()]
    }

    fn inspect_network_args(&self, name: &str) -> Vec<String> {
        vec!["network".into(), "inspect".into(), name.into()]
    }

    fn inspect_volume_args(&self, name: &str) -> Vec<String> {
        vec!["volume".into(), "inspect".into(), name.into()]
    }

    fn inspect_image_args(&self, reference: &str) -> Vec<String> {
        vec![
            "inspect".into(),
            "--format".into(),
            "json".into(),
            reference.into(),
        ]
    }

    fn build_args(&self, spec: &ComposeServiceBuild, image_name: &str) -> Vec<String> {
        let mut args = vec!["build".into(), "-t".into(), image_name.to_string()];
        if let Some(ref f) = spec.containerfile {
            args.extend(["-f".into(), f.clone()]);
        }
        args.push(spec.context.as_deref().unwrap_or(".").to_string());
        args
    }

    fn security_args(&self, profile: &SecurityProfile) -> Vec<String> {
        let mut args = Vec::new();
        if profile.read_only_root {
            args.push("--read-only".into());
        }
        if let Some(seccomp) = &profile.seccomp {
            args.extend(["--security-opt".into(), format!("seccomp={}", seccomp)]);
        }
        if profile.no_new_privileges {
            // Docker accepts both forms; use `:true` to match the
            // canonical compose-spec example.
            args.extend(["--security-opt".into(), "no-new-privileges:true".into()]);
        }
        args
    }

    fn parse_list_output(&self, stdout: &str) -> Result<Vec<ContainerInfo>> {
        let entries: Vec<DockerListEntry> = stdout
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        Ok(entries
            .into_iter()
            .map(|e| {
                let mut labels = HashMap::new();
                if let Some(map) = e.labels.as_object() {
                    for (k, v) in map {
                        labels.insert(k.clone(), v.as_str().unwrap_or("").to_string());
                    }
                } else if let Some(s) = e.labels.as_str() {
                    // Handle comma-separated labels if necessary
                    for pair in s.split(',') {
                        let mut parts = pair.splitn(2, '=');
                        if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                            labels.insert(k.to_string(), v.to_string());
                        }
                    }
                }

                ContainerInfo {
                    id: e.id,
                    name: e.names.first().cloned().unwrap_or_default(),
                    image: e.image,
                    status: e.status,
                    ports: e.ports,
                    labels,
                    created: e.created,
                    ip_address: String::new(),
                }
            })
            .collect())
    }

    fn parse_inspect_output(&self, stdout: &str) -> Result<ContainerInfo> {
        let entries: Vec<DockerInspectOutput> = serde_json::from_str(stdout)?;
        let e = entries
            .into_iter()
            .next()
            .ok_or_else(|| ComposeError::NotFound("Inspect output empty".into()))?;

        let mut ip_address = String::new();
        if let Some(settings) = &e.network_settings {
            if !settings.ip_address.is_empty() {
                ip_address = settings.ip_address.clone();
            } else {
                // Try to get from first network
                if let Some(net) = settings.networks.values().next() {
                    ip_address = net.ip_address.clone();
                }
            }
        }

        Ok(ContainerInfo {
            id: e.id,
            name: e.name,
            image: e.config.image,
            status: e.state.status,
            ports: vec![],
            labels: e.config.labels,
            created: e.created,
            ip_address,
        })
    }

    fn parse_list_images_output(&self, stdout: &str) -> Result<Vec<ImageInfo>> {
        let entries: Vec<DockerImageEntry> = stdout
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        Ok(entries
            .into_iter()
            .map(|e| ImageInfo {
                id: e.id,
                repository: e.repository,
                tag: e.tag,
                size: e.size,
                created: e.created,
            })
            .collect())
    }

    fn parse_container_id(&self, stdout: &str) -> Result<String> {
        Ok(stdout.trim().to_string())
    }
}
