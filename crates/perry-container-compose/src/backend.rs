use crate::error::{ComposeError, Result};
use crate::types::{
    ComposeNetwork, ComposeServiceBuild, ComposeVolume, ContainerHandle, ContainerInfo,
    ContainerLogs, ContainerSpec, ImageInfo,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendProbeResult {
    pub name: String,
    pub available: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Default)]
pub struct SecurityProfile {
    pub read_only_root: bool,
    /// Path to a seccomp JSON profile, or the literal string `"default"`
    /// to use the runtime's default profile. Emitted as
    /// `--security-opt seccomp=<value>`. Maps to the user's
    /// `security_opt: ["seccomp=..."]` entries on `ComposeService`.
    pub seccomp: Option<String>,
    /// `--security-opt no-new-privileges`. SUID/SGID binaries inside
    /// the container can't gain privileges via execve. Maps to the
    /// user's `security_opt: ["no-new-privileges"]` (or `:true` /
    /// `=true`) entries.
    pub no_new_privileges: bool,
}

impl SecurityProfile {
    /// Parse a `security_opt: Vec<String>` from `ComposeService` into
    /// the structured `SecurityProfile`. Pre-fix the engine had a
    /// `// Could be parsed from security_opt` TODO and silently
    /// dropped these fields — a security regression where users
    /// thought they were hardening containers but the flags never
    /// reached the runtime.
    ///
    /// Recognised entries (compose-spec §service.security_opt):
    /// - `"seccomp=<path>"` / `"seccomp:<path>"` → `seccomp`
    /// - `"seccomp=default"` → `seccomp = Some("default")`
    /// - `"no-new-privileges"` / `"no-new-privileges:true"` /
    ///   `"no-new-privileges=true"` → `no_new_privileges = true`
    ///
    /// Unrecognised entries are ignored (left for the caller's
    /// future support; `tracing::warn!` could be added if desired).
    pub fn merge_security_opt(&mut self, security_opt: &[String]) {
        for opt in security_opt {
            // seccomp=<path> or seccomp:<path>
            if let Some(rest) = opt
                .strip_prefix("seccomp=")
                .or_else(|| opt.strip_prefix("seccomp:"))
            {
                self.seccomp = Some(rest.to_string());
                continue;
            }
            // no-new-privileges, no-new-privileges:true, no-new-privileges=true
            if opt == "no-new-privileges"
                || opt == "no-new-privileges:true"
                || opt == "no-new-privileges=true"
            {
                self.no_new_privileges = true;
                continue;
            }
        }
    }
}

#[async_trait]
pub trait ContainerBackend: Send + Sync {
    fn backend_name(&self) -> &str;

    /// What this backend can do. The engine reads this to decide which
    /// `ContainerSpec` fields to drop / translate / hard-reject before
    /// calling `run_with_security`. Default returns the Docker baseline
    /// (everything supported); concrete backends should override.
    fn capabilities(&self) -> &'static crate::capabilities::BackendCapabilities {
        &crate::capabilities::BackendCapabilities::DOCKER
    }

    async fn check_available(&self) -> Result<()>;
    async fn run(&self, spec: &ContainerSpec) -> Result<ContainerHandle>;
    async fn create(&self, spec: &ContainerSpec) -> Result<ContainerHandle>;
    async fn start(&self, id: &str) -> Result<()>;
    async fn stop(&self, id: &str, timeout: Option<u32>) -> Result<()>;
    async fn remove(&self, id: &str, force: bool) -> Result<()>;
    async fn list(&self, all: bool) -> Result<Vec<ContainerInfo>>;
    async fn inspect(&self, id: &str) -> Result<ContainerInfo>;
    async fn logs(&self, id: &str, tail: Option<u32>) -> Result<ContainerLogs>;
    async fn exec(
        &self,
        id: &str,
        cmd: &[String],
        env: Option<&HashMap<String, String>>,
        workdir: Option<&str>,
    ) -> Result<ContainerLogs>;
    async fn pull_image(&self, reference: &str) -> Result<()>;
    async fn list_images(&self) -> Result<Vec<ImageInfo>>;
    async fn remove_image(&self, reference: &str, force: bool) -> Result<()>;
    async fn create_network(&self, name: &str, config: &ComposeNetwork) -> Result<()>;
    async fn remove_network(&self, name: &str) -> Result<()>;
    async fn create_volume(&self, name: &str, config: &ComposeVolume) -> Result<()>;
    async fn remove_volume(&self, name: &str) -> Result<()>;
    async fn inspect_network(&self, name: &str) -> Result<()>;
    async fn inspect_volume(&self, name: &str) -> Result<()>;
    async fn inspect_image(&self, reference: &str) -> Result<ImageInfo>;
    async fn build(&self, spec: &ComposeServiceBuild, image_name: &str) -> Result<()>;
    async fn run_with_security(
        &self,
        spec: &ContainerSpec,
        profile: &SecurityProfile,
    ) -> Result<ContainerHandle>;
    /// Wait for a container to exit and return its exit code.
    async fn wait(&self, id: &str) -> Result<i32>;
}

pub trait CliProtocol: Send + Sync {
    fn subcommand_prefix(&self) -> Option<&str> {
        None
    }

    /// What this backend can do. Drives the spec-normalization pass that
    /// keeps cross-backend behavior deterministic — see
    /// `crate::capabilities` for the architecture writeup.
    ///
    /// Default impl returns `BackendCapabilities::DOCKER` (the
    /// "everything supported" baseline) — protocols that diverge from
    /// the Docker reference override this.
    fn capabilities(&self) -> &'static crate::capabilities::BackendCapabilities {
        &crate::capabilities::BackendCapabilities::DOCKER
    }

    fn run_args(&self, spec: &ContainerSpec) -> Vec<String>;
    fn create_args(&self, spec: &ContainerSpec) -> Vec<String>;
    fn start_args(&self, id: &str) -> Vec<String>;
    fn stop_args(&self, id: &str, timeout: Option<u32>) -> Vec<String>;
    fn remove_args(&self, id: &str, force: bool) -> Vec<String>;
    fn list_args(&self, all: bool) -> Vec<String>;
    fn inspect_args(&self, id: &str) -> Vec<String>;
    fn logs_args(&self, id: &str, tail: Option<u32>) -> Vec<String>;
    fn exec_args(
        &self,
        id: &str,
        cmd: &[String],
        env: Option<&HashMap<String, String>>,
        workdir: Option<&str>,
    ) -> Vec<String>;
    fn pull_image_args(&self, reference: &str) -> Vec<String>;
    fn list_images_args(&self) -> Vec<String>;
    fn remove_image_args(&self, reference: &str, force: bool) -> Vec<String>;
    fn create_network_args(&self, name: &str, config: &ComposeNetwork) -> Vec<String>;
    fn remove_network_args(&self, name: &str) -> Vec<String>;
    fn create_volume_args(&self, name: &str, config: &ComposeVolume) -> Vec<String>;
    fn remove_volume_args(&self, name: &str) -> Vec<String>;
    fn inspect_network_args(&self, name: &str) -> Vec<String>;
    fn inspect_volume_args(&self, name: &str) -> Vec<String>;
    fn inspect_image_args(&self, reference: &str) -> Vec<String>;
    fn build_args(&self, spec: &ComposeServiceBuild, image_name: &str) -> Vec<String>;
    fn security_args(&self, profile: &SecurityProfile) -> Vec<String>;

    fn parse_list_output(&self, stdout: &str) -> Result<Vec<ContainerInfo>>;
    fn parse_inspect_output(&self, stdout: &str) -> Result<ContainerInfo>;
    fn parse_list_images_output(&self, stdout: &str) -> Result<Vec<ImageInfo>>;
    fn parse_container_id(&self, stdout: &str) -> Result<String>;
}

#[derive(Debug, Deserialize)]
struct DockerListEntry {
    #[serde(rename = "ID", alias = "Id", default)]
    id: String,
    #[serde(rename = "Names", default)]
    names: Vec<String>,
    #[serde(rename = "Image", default)]
    image: String,
    #[serde(rename = "Status", alias = "State", default)]
    status: String,
    #[serde(rename = "Ports", default)]
    ports: Vec<String>,
    #[serde(rename = "Labels", default)]
    labels: serde_json::Value,
    #[serde(rename = "Created", alias = "CreatedAt", default)]
    created: String,
}

#[derive(Debug, Deserialize)]
struct DockerInspectOutput {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Config")]
    config: DockerInspectConfig,
    #[serde(rename = "State")]
    state: DockerInspectState,
    #[serde(rename = "Created")]
    created: String,
    #[serde(rename = "NetworkSettings", default)]
    network_settings: Option<DockerInspectNetworkSettings>,
}

#[derive(Debug, Deserialize)]
struct DockerInspectConfig {
    #[serde(rename = "Image")]
    image: String,
    #[serde(rename = "Labels", default)]
    labels: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct DockerInspectState {
    #[serde(rename = "Status")]
    status: String,
}

#[derive(Debug, Deserialize)]
struct DockerInspectNetworkSettings {
    #[serde(rename = "IPAddress", default)]
    ip_address: String,
    #[serde(rename = "Networks", default)]
    networks: HashMap<String, DockerInspectNetwork>,
}

#[derive(Debug, Deserialize)]
struct DockerInspectNetwork {
    #[serde(rename = "IPAddress", default)]
    ip_address: String,
}

#[derive(Debug, Deserialize)]
struct DockerImageEntry {
    #[serde(rename = "ID", alias = "Id", default)]
    id: String,
    #[serde(rename = "Repositories", alias = "Repository", default)]
    repository: String,
    #[serde(rename = "Tag", default)]
    tag: String,
    #[serde(rename = "Size", default)]
    size: u64,
    #[serde(rename = "Created", alias = "CreatedAt", default)]
    created: String,
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

// ====================== apple/container ======================
//
// apple/container (https://github.com/apple/container) is Apple's native
// macOS container runtime. It speaks an OCI-compatible spec but its CLI
// surface diverges from `docker` on several axes that matter for an
// orchestrator. The pre-v0.5.374 implementation delegated 80% of arg
// construction back to DockerProtocol, which produced silent breakage
// on common ops (`pull`, `images`, `inspect`, `logs --tail` etc.). Each
// divergence below is annotated with the CLI evidence; verified against
// `container CLI version 0.12.0`.
//
// **Subcommand differences**:
//
// - Image ops live under `image` (`container image pull`,
//   `container image list`, `container image delete`,
//   `container image inspect`). Docker exposes them at top level
//   (`docker pull`, `docker images`, `docker rmi`, `docker inspect`).
//
// - Container list is `list` / `ls` — there is **no `ps`** alias.
//
// - Container removal is `delete` (with `rm` accepted as alias). Volume
//   and network removal both use `delete`.
//
// **Flag differences**:
//
// - `logs` uses `-n <N>`, not `--tail <N>`.
// - `inspect` outputs JSON natively — does **not** accept `--format`.
// - `volume create` does **not** accept `--driver` (driver model is
//   implicit; only `--label`, `--opt`, `-s` are valid).
// - `run` does **not** support `--privileged`, `--security-opt`,
//   `--restart`, `--ipc`, or `--pid`. Apple silently warns + may reject.
// - `run` requires explicit `--detach` for the orchestrator's
//   "create-and-start, return ID" semantics. Pre-fix the engine
//   blocked on the container's main process.
// - JSON shapes diverge: list / inspect / image-list each have their
//   own field naming (`configuration.id`, `image.reference`, etc.).
//
// **Apple-only flags we propagate when set on `ContainerSpec` (extension
// fields are forward-compatible no-ops on Docker)**:
//
// - `--arch` / `--os` / `--platform` for cross-arch image pulls.
// - `--rosetta` for x86_64-on-arm64 translation.
// - `--virtualization` for nested virt.
// - `--ssh` for SSH agent forwarding.
//
// These aren't on `ContainerSpec` today; the orchestrator wires them in
// only on apple/container until they're standardized.
pub struct AppleContainerProtocol;

impl CliProtocol for AppleContainerProtocol {
    fn capabilities(&self) -> &'static crate::capabilities::BackendCapabilities {
        &crate::capabilities::BackendCapabilities::APPLE
    }

    fn run_args(&self, spec: &ContainerSpec) -> Vec<String> {
        // `run` is foreground by default. The orchestrator needs the ID
        // back so it can proceed to the next service — emit `--detach`.
        let mut args = vec!["run".into(), "--detach".into()];

        if spec.rm.unwrap_or(false) {
            args.push("--rm".into());
        }
        if let Some(name) = &spec.name {
            args.extend(["--name".into(), name.clone()]);
        }
        if let Some(network) = &spec.network {
            args.extend(["--network".into(), network.clone()]);
        }
        // Service-key network alias — apple/container 0.12+ accepts
        // `--network-alias` with the same semantics as docker. On older
        // alpha builds this flag was a no-op rather than a hard error,
        // so we always emit it; the engine still falls back to
        // `container_name` cross-resolution.
        if let Some(aliases) = &spec.network_aliases {
            for alias in aliases {
                args.extend(["--network-alias".into(), alias.clone()]);
            }
        }
        for port in spec.ports.as_ref().iter().flat_map(|v| v.iter()) {
            args.extend(["-p".into(), port.clone()]);
        }
        for vol in spec.volumes.as_ref().iter().flat_map(|v| v.iter()) {
            // apple/container's `-v` accepts the same `host:container[:ro]`
            // syntax docker uses, plus `volume_name:container` for named
            // volumes. The compose engine emits both shapes.
            args.extend(["-v".into(), vol.clone()]);
        }
        for (k, v) in spec.env.as_ref().iter().flat_map(|m| m.iter()) {
            args.extend(["-e".into(), format!("{k}={v}")]);
        }
        for (k, v) in spec.labels.as_ref().iter().flat_map(|m| m.iter()) {
            args.extend(["--label".into(), format!("{k}={v}")]);
        }
        if spec.read_only.unwrap_or(false) {
            args.push("--read-only".into());
        }
        // `--privileged` is intentionally **not** emitted: apple/container
        // doesn't support it (Linux containers run inside an Apple-VM, so
        // host-privilege escalation isn't a concept). Pre-fix we'd emit
        // it unconditionally, which produced confusing CLI errors.
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
            // apple/container's `--entrypoint <cmd>` takes a single
            // string, same shape as docker's. The engine joins multi-arg
            // entrypoints with spaces (matching DockerProtocol).
            args.extend(["--entrypoint".into(), ep.join(" ")]);
        }
        args.push(spec.image.clone());
        for c in spec.cmd.as_ref().iter().flat_map(|v| v.iter()) {
            args.push(c.clone());
        }
        args
    }

    fn create_args(&self, spec: &ContainerSpec) -> Vec<String> {
        // apple/container has a real `create` subcommand. Build the same
        // arg shape as `run_args` minus `--detach` (create doesn't run).
        let mut args = vec!["create".into()];
        if let Some(name) = &spec.name {
            args.extend(["--name".into(), name.clone()]);
        }
        if let Some(network) = &spec.network {
            args.extend(["--network".into(), network.clone()]);
        }
        if let Some(aliases) = &spec.network_aliases {
            for alias in aliases {
                args.extend(["--network-alias".into(), alias.clone()]);
            }
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
        if spec.read_only.unwrap_or(false) {
            args.push("--read-only".into());
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
            args.extend(["--entrypoint".into(), ep.join(" ")]);
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
        // apple/container exposes both `-t` (short) and `--time` (long).
        // Stick with `--time` for symmetry with DockerProtocol.
        let mut args = vec!["stop".into()];
        if let Some(t) = timeout {
            args.extend(["--time".into(), t.to_string()]);
        }
        args.push(id.into());
        args
    }

    fn remove_args(&self, id: &str, force: bool) -> Vec<String> {
        // Use `delete` (the canonical name); `rm` is accepted as alias.
        let mut args = vec!["delete".into()];
        if force {
            args.push("--force".into());
        }
        args.push(id.into());
        args
    }

    fn list_args(&self, all: bool) -> Vec<String> {
        // apple/container has `list` / `ls` — there is **no `ps` alias**.
        let mut args = vec!["list".into(), "--format".into(), "json".into()];
        if all {
            args.push("--all".into());
        }
        args
    }

    fn inspect_args(&self, id: &str) -> Vec<String> {
        // apple/container's `inspect` outputs JSON natively. It does
        // **not** accept `--format`. Pre-fix we'd emit `--format json`
        // and apple would reject it as an unknown flag.
        vec!["inspect".into(), id.into()]
    }

    fn logs_args(&self, id: &str, tail: Option<u32>) -> Vec<String> {
        // apple/container uses `-n <N>`, not docker's `--tail <N>`.
        let mut args = vec!["logs".into()];
        if let Some(t) = tail {
            args.extend(["-n".into(), t.to_string()]);
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
        // apple/container's `exec` accepts the same flags as docker
        // for the subset we use: `-w/--workdir/--cwd`, `-e KEY=VAL`.
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
        // apple/container scopes image ops under the `image` subcommand:
        // `container image pull <ref>` (NOT `container pull <ref>`).
        vec!["image".into(), "pull".into(), reference.into()]
    }

    fn list_images_args(&self) -> Vec<String> {
        vec![
            "image".into(),
            "list".into(),
            "--format".into(),
            "json".into(),
        ]
    }

    fn remove_image_args(&self, reference: &str, force: bool) -> Vec<String> {
        let mut args = vec!["image".into(), "delete".into()];
        if force {
            args.push("--force".into());
        }
        args.push(reference.into());
        args
    }

    fn create_network_args(&self, name: &str, config: &ComposeNetwork) -> Vec<String> {
        // apple/container's network plugin requires `container system
        // start` to be active. The args themselves are: `network create
        // <name>` plus optional labels. apple/container does **not**
        // honor docker's `--driver bridge` (the driver model is implicit
        // in the apple-network plugin) — drop the flag if set.
        let mut args = vec!["network".into(), "create".into()];
        if let Some(lbls) = &config.labels {
            for (k, v) in lbls.to_map() {
                args.extend(["--label".into(), format!("{k}={v}")]);
            }
        }
        args.push(name.into());
        args
    }

    fn remove_network_args(&self, name: &str) -> Vec<String> {
        vec!["network".into(), "delete".into(), name.into()]
    }

    fn create_volume_args(&self, name: &str, config: &ComposeVolume) -> Vec<String> {
        // apple/container's `volume create` accepts only `--label`,
        // `--opt`, and `-s <size>`. Docker's `--driver` is **not**
        // accepted; silently drop it if set on the spec (apple's volume
        // model is local-only, so a driver flag has no meaning).
        let mut args = vec!["volume".into(), "create".into()];
        if let Some(lbls) = &config.labels {
            for (k, v) in lbls.to_map() {
                args.extend(["--label".into(), format!("{k}={v}")]);
            }
        }
        args.push(name.into());
        args
    }

    fn remove_volume_args(&self, name: &str) -> Vec<String> {
        vec!["volume".into(), "delete".into(), name.into()]
    }

    fn inspect_network_args(&self, name: &str) -> Vec<String> {
        vec!["network".into(), "inspect".into(), name.into()]
    }

    fn inspect_volume_args(&self, name: &str) -> Vec<String> {
        vec!["volume".into(), "inspect".into(), name.into()]
    }

    fn inspect_image_args(&self, reference: &str) -> Vec<String> {
        // apple/container scopes image inspect under the `image`
        // subcommand and outputs JSON natively (no `--format`).
        vec!["image".into(), "inspect".into(), reference.into()]
    }

    fn build_args(&self, spec: &ComposeServiceBuild, image_name: &str) -> Vec<String> {
        // apple/container's `build` accepts `-t <name>` and `-f <file>`
        // with the same semantics as docker. The default output is
        // `type=oci` which produces an image addressable by tag.
        let mut args = vec!["build".into(), "-t".into(), image_name.to_string()];
        if let Some(ref f) = spec.containerfile {
            args.extend(["-f".into(), f.clone()]);
        }
        args.push(spec.context.as_deref().unwrap_or(".").to_string());
        args
    }

    fn security_args(&self, profile: &SecurityProfile) -> Vec<String> {
        // apple/container does **not** support `--security-opt seccomp=`.
        // Honor only the flags it understands: `--read-only`. Seccomp
        // profiles are silently dropped — the orchestrator surfaces a
        // warning at the engine layer instead of producing an arg the
        // CLI rejects.
        let mut args = Vec::new();
        if profile.read_only_root {
            args.push("--read-only".into());
        }
        args
    }

    fn parse_list_output(&self, stdout: &str) -> Result<Vec<ContainerInfo>> {
        // apple/container's `list --format json` returns a JSON array,
        // **not** NDJSON. Each entry follows apple's snapshot shape:
        //
        //   [{
        //     "configuration": { "id": "...", "image": { "reference": "..." } },
        //     "status": "running",
        //     "networks": [{ "address": "..." }]
        //   }]
        //
        // The exact field set varies between releases; use defensive
        // serde with sensible aliases to track multiple shapes without
        // breaking on a CLI version bump. We also fall back to the
        // Docker shape when a runtime presents itself as apple-compatible
        // but emits docker-shaped JSON.
        let trimmed = stdout.trim();
        if trimmed.is_empty() || trimmed == "[]" {
            // Explicitly short-circuit `[]` — without this we'd fall
            // through to the docker parser, whose `stdout.lines()` +
            // `serde_json::from_str::<DockerListEntry>("[]")` succeeds
            // with all `#[serde(default)]` fields empty, producing one
            // bogus empty ContainerInfo.
            return Ok(Vec::new());
        }
        if let Ok(entries) = serde_json::from_str::<Vec<AppleListEntry>>(trimmed) {
            // Defensive: every apple-shape field is `#[serde(default)]`
            // so a docker-shaped JSON parses successfully but with all
            // fields empty. Detect that and fall through to the docker
            // parser.
            if entries.iter().any(|e| !e.configuration.id.is_empty()) {
                return Ok(entries.into_iter().map(AppleListEntry::into_info).collect());
            }
        }
        // Fallback: maybe the runtime is Docker-shaped. Try NDJSON first
        // (docker), then a JSON array of docker-shaped entries.
        DockerProtocol.parse_list_output(stdout)
    }

    fn parse_inspect_output(&self, stdout: &str) -> Result<ContainerInfo> {
        let trimmed = stdout.trim();
        if trimmed.is_empty() {
            return Err(ComposeError::NotFound("Inspect output empty".into()));
        }
        if let Ok(entries) = serde_json::from_str::<Vec<AppleInspectEntry>>(trimmed) {
            if let Some(e) = entries.into_iter().next() {
                // Same defensive check as parse_list_output: a docker-
                // shaped JSON parses cleanly through serde-default and
                // produces empty fields. Reject if id+image are empty.
                if !e.configuration.id.is_empty() || !e.configuration.image.reference.is_empty() {
                    return Ok(e.into_info());
                }
            }
        }
        // Fall back to the Docker shape if apple-shape parse failed or
        // produced an empty info struct.
        DockerProtocol.parse_inspect_output(stdout)
    }

    fn parse_list_images_output(&self, stdout: &str) -> Result<Vec<ImageInfo>> {
        let trimmed = stdout.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }
        if let Ok(entries) = serde_json::from_str::<Vec<AppleImageEntry>>(trimmed) {
            // Same defensive check: docker shape may parse with all
            // apple fields empty. Require at least one populated.
            if entries
                .iter()
                .any(|e| !e.reference.is_empty() || !e.id.is_empty() || !e.name.is_empty())
            {
                return Ok(entries
                    .into_iter()
                    .map(AppleImageEntry::into_info)
                    .collect());
            }
        }
        DockerProtocol.parse_list_images_output(stdout)
    }

    fn parse_container_id(&self, stdout: &str) -> Result<String> {
        // apple/container `run --detach` prints the container ID to
        // stdout, same as docker. Strip whitespace.
        Ok(stdout.trim().to_string())
    }
}

// ---- apple/container JSON shapes ----
//
// These shapes are reverse-engineered from the apple/container 0.12
// CLI output and the `Containerization` Swift module's serde derive
// pattern. Field names use camelCase + snake_case aliases because apple
// has flipped between conventions across patch releases. `serde(default)`
// on every field keeps the parser robust against shape drift.

#[derive(Debug, Deserialize)]
struct AppleListEntry {
    #[serde(default)]
    configuration: AppleListConfig,
    #[serde(default)]
    status: String,
    #[serde(default)]
    networks: Vec<AppleNetworkEntry>,
}

#[derive(Debug, Default, Deserialize)]
struct AppleListConfig {
    #[serde(default, alias = "ID")]
    id: String,
    #[serde(default)]
    image: AppleImageRef,
    #[serde(default, alias = "name")]
    hostname: String,
    #[serde(default)]
    labels: HashMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
struct AppleImageRef {
    #[serde(default)]
    reference: String,
}

#[derive(Debug, Default, Deserialize)]
struct AppleNetworkEntry {
    #[serde(default, alias = "ip", alias = "ipAddress", alias = "ip_address")]
    address: String,
}

impl AppleListEntry {
    fn into_info(self) -> ContainerInfo {
        ContainerInfo {
            id: self.configuration.id.clone(),
            // apple/container doesn't separate "name" and "id" the same
            // way docker does. The hostname is the closest analogue.
            name: if self.configuration.hostname.is_empty() {
                self.configuration.id
            } else {
                self.configuration.hostname
            },
            image: self.configuration.image.reference,
            status: self.status,
            ports: Vec::new(),
            labels: self.configuration.labels,
            created: String::new(),
            ip_address: self
                .networks
                .into_iter()
                .next()
                .map(|n| n.address)
                .unwrap_or_default(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct AppleInspectEntry {
    #[serde(default)]
    configuration: AppleListConfig,
    #[serde(default)]
    status: String,
    #[serde(default)]
    networks: Vec<AppleNetworkEntry>,
}

impl AppleInspectEntry {
    fn into_info(self) -> ContainerInfo {
        AppleListEntry {
            configuration: self.configuration,
            status: self.status,
            networks: self.networks,
        }
        .into_info()
    }
}

#[derive(Debug, Default, Deserialize)]
struct AppleImageEntry {
    // apple/container's image-list JSON uses a "reference" field that
    // bundles registry/repo/tag (`docker.io/library/alpine:latest`).
    // Some releases also emit `name` + `tag` separately.
    #[serde(default)]
    reference: String,
    #[serde(default, alias = "ID")]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    tag: String,
    #[serde(default)]
    size: u64,
    #[serde(default, alias = "createdAt", alias = "created_at")]
    created: String,
}

impl AppleImageEntry {
    fn into_info(self) -> ImageInfo {
        let (repository, tag) = if !self.reference.is_empty() {
            split_image_reference(&self.reference)
        } else if !self.name.is_empty() {
            (
                self.name.clone(),
                if self.tag.is_empty() {
                    "latest".to_string()
                } else {
                    self.tag.clone()
                },
            )
        } else {
            (String::new(), String::new())
        };
        ImageInfo {
            id: self.id,
            repository,
            tag,
            size: self.size,
            created: self.created,
        }
    }
}

/// Splits `registry/repo:tag` into `(repository, tag)`. The tag defaults
/// to `latest` when omitted; digests (`@sha256:...`) are preserved as
/// the tag value to match docker's behavior.
fn split_image_reference(reference: &str) -> (String, String) {
    if let Some(at_idx) = reference.rfind('@') {
        // Digest reference — `repo@sha256:...`
        let (repo, digest) = reference.split_at(at_idx);
        return (repo.to_string(), digest.trim_start_matches('@').to_string());
    }
    // Find the LAST `:` after the LAST `/` — registry hostnames may
    // contain `:port` which is not a tag.
    let after_slash = reference.rfind('/').map(|i| i + 1).unwrap_or(0);
    if let Some(colon) = reference[after_slash..].rfind(':') {
        let abs_colon = after_slash + colon;
        return (
            reference[..abs_colon].to_string(),
            reference[abs_colon + 1..].to_string(),
        );
    }
    (reference.to_string(), "latest".to_string())
}

pub struct LimaProtocol {
    pub instance: String,
}

impl CliProtocol for LimaProtocol {
    fn capabilities(&self) -> &'static crate::capabilities::BackendCapabilities {
        &crate::capabilities::BackendCapabilities::LIMA
    }

    fn run_args(&self, spec: &ContainerSpec) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.run_args(spec));
        args
    }
    fn create_args(&self, spec: &ContainerSpec) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.create_args(spec));
        args
    }
    fn start_args(&self, id: &str) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.start_args(id));
        args
    }
    fn stop_args(&self, id: &str, timeout: Option<u32>) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.stop_args(id, timeout));
        args
    }
    fn remove_args(&self, id: &str, force: bool) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.remove_args(id, force));
        args
    }
    fn list_args(&self, all: bool) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.list_args(all));
        args
    }
    fn inspect_args(&self, id: &str) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.inspect_args(id));
        args
    }
    fn logs_args(&self, id: &str, tail: Option<u32>) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.logs_args(id, tail));
        args
    }
    fn exec_args(
        &self,
        id: &str,
        cmd: &[String],
        env: Option<&HashMap<String, String>>,
        workdir: Option<&str>,
    ) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.exec_args(id, cmd, env, workdir));
        args
    }
    fn pull_image_args(&self, reference: &str) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.pull_image_args(reference));
        args
    }
    fn list_images_args(&self) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.list_images_args());
        args
    }
    fn remove_image_args(&self, reference: &str, force: bool) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.remove_image_args(reference, force));
        args
    }
    fn create_network_args(&self, name: &str, config: &ComposeNetwork) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.create_network_args(name, config));
        args
    }
    fn remove_network_args(&self, name: &str) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.remove_network_args(name));
        args
    }
    fn create_volume_args(&self, name: &str, config: &ComposeVolume) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.create_volume_args(name, config));
        args
    }
    fn remove_volume_args(&self, name: &str) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.remove_volume_args(name));
        args
    }
    fn inspect_network_args(&self, name: &str) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.inspect_network_args(name));
        args
    }
    fn inspect_volume_args(&self, name: &str) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.inspect_volume_args(name));
        args
    }
    fn inspect_image_args(&self, reference: &str) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.inspect_image_args(reference));
        args
    }
    fn build_args(&self, spec: &ComposeServiceBuild, image_name: &str) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.build_args(spec, image_name));
        args
    }
    fn security_args(&self, profile: &SecurityProfile) -> Vec<String> {
        // Return only the nerdctl flags, the caller (run_with_security) will insert them
        // into the already prefixed run_args.
        DockerProtocol.security_args(profile)
    }
    fn parse_list_output(&self, stdout: &str) -> Result<Vec<ContainerInfo>> {
        DockerProtocol.parse_list_output(stdout)
    }
    fn parse_inspect_output(&self, stdout: &str) -> Result<ContainerInfo> {
        DockerProtocol.parse_inspect_output(stdout)
    }
    fn parse_list_images_output(&self, stdout: &str) -> Result<Vec<ImageInfo>> {
        DockerProtocol.parse_list_images_output(stdout)
    }
    fn parse_container_id(&self, stdout: &str) -> Result<String> {
        DockerProtocol.parse_container_id(stdout)
    }
}

pub struct CliBackend {
    pub bin: PathBuf,
    pub protocol: Box<dyn CliProtocol>,
}

impl CliBackend {
    pub fn new(bin: PathBuf, protocol: Box<dyn CliProtocol>) -> Self {
        Self { bin, protocol }
    }

    async fn exec_raw(&self, args: &[String]) -> Result<(String, String)> {
        // Per-op timeout. Pre-fix `Command::output().await` could hang
        // forever — Docker daemon hangs are common in CI and shipping
        // a forever-blocking primitive in a production orchestrator
        // is not acceptable. Default 5 minutes is generous (image pulls
        // need the headroom); override per-process via
        // `PERRY_CONTAINER_OP_TIMEOUT_SECS=<N>` env var.
        let timeout_secs = std::env::var("PERRY_CONTAINER_OP_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(300);
        let timeout = Duration::from_secs(timeout_secs);

        let fut = Command::new(&self.bin).args(args).output();
        let output = match tokio::time::timeout(timeout, fut).await {
            Ok(Ok(out)) => out,
            Ok(Err(e)) => return Err(ComposeError::IoError(e)),
            Err(_) => {
                return Err(ComposeError::BackendError {
                    code: -1,
                    message: format!(
                        "container CLI `{}` hung for {}s; aborted (configure via PERRY_CONTAINER_OP_TIMEOUT_SECS)",
                        self.bin.display(),
                        timeout_secs
                    ),
                });
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if output.status.success() {
            Ok((stdout, stderr))
        } else {
            // Truncate stderr in error messages — a multi-MB image-pull
            // failure log shouldn't end up verbatim in a user-facing
            // Error.message. The full output is still on the daemon's
            // logs if the user needs to investigate.
            const STDERR_TRUNCATE_LIMIT: usize = 4096;
            let truncated = if stderr.len() > STDERR_TRUNCATE_LIMIT {
                format!(
                    "{}... [truncated, {} bytes total]",
                    &stderr[..STDERR_TRUNCATE_LIMIT],
                    stderr.len()
                )
            } else {
                stderr
            };
            Err(ComposeError::BackendError {
                code: output.status.code().unwrap_or(-1),
                message: truncated,
            })
        }
    }
}

#[async_trait]
impl ContainerBackend for CliBackend {
    fn backend_name(&self) -> &str {
        self.bin
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
    }

    /// Forward to the underlying protocol's capability table. The
    /// engine + normalization layer above read this; default impl on
    /// the trait would always return `DOCKER` regardless of the actual
    /// runtime, which would silently emit `--privileged` to apple.
    fn capabilities(&self) -> &'static crate::capabilities::BackendCapabilities {
        self.protocol.capabilities()
    }

    async fn check_available(&self) -> Result<()> {
        Command::new(&self.bin)
            .arg("--version")
            .output()
            .await
            .map_err(ComposeError::IoError)
            .map(|_| ())
    }

    async fn run(&self, spec: &ContainerSpec) -> Result<ContainerHandle> {
        let args = self.protocol.run_args(spec);
        let (stdout, _) = self.exec_raw(&args).await?;
        let id = self.protocol.parse_container_id(&stdout)?;
        Ok(ContainerHandle {
            id,
            name: spec.name.clone(),
        })
    }

    async fn create(&self, spec: &ContainerSpec) -> Result<ContainerHandle> {
        let args = self.protocol.create_args(spec);
        let (stdout, _) = self.exec_raw(&args).await?;
        let id = self.protocol.parse_container_id(&stdout)?;
        Ok(ContainerHandle {
            id,
            name: spec.name.clone(),
        })
    }

    async fn start(&self, id: &str) -> Result<()> {
        let args = self.protocol.start_args(id);
        self.exec_raw(&args).await.map(|_| ())
    }

    async fn stop(&self, id: &str, timeout: Option<u32>) -> Result<()> {
        let args = self.protocol.stop_args(id, timeout);
        self.exec_raw(&args).await.map(|_| ())
    }

    async fn remove(&self, id: &str, force: bool) -> Result<()> {
        let args = self.protocol.remove_args(id, force);
        self.exec_raw(&args).await.map(|_| ())
    }

    async fn list(&self, all: bool) -> Result<Vec<ContainerInfo>> {
        let args = self.protocol.list_args(all);
        let (stdout, _) = self.exec_raw(&args).await?;
        self.protocol.parse_list_output(&stdout)
    }

    async fn inspect(&self, id: &str) -> Result<ContainerInfo> {
        let args = self.protocol.inspect_args(id);
        let (stdout, _) = self.exec_raw(&args).await?;
        self.protocol.parse_inspect_output(&stdout)
    }

    async fn logs(&self, id: &str, tail: Option<u32>) -> Result<ContainerLogs> {
        let args = self.protocol.logs_args(id, tail);
        let (stdout, stderr) = self.exec_raw(&args).await?;
        Ok(ContainerLogs { stdout, stderr })
    }

    async fn exec(
        &self,
        id: &str,
        cmd: &[String],
        env: Option<&HashMap<String, String>>,
        workdir: Option<&str>,
    ) -> Result<ContainerLogs> {
        let args = self.protocol.exec_args(id, cmd, env, workdir);
        let (stdout, stderr) = self.exec_raw(&args).await?;
        Ok(ContainerLogs { stdout, stderr })
    }

    async fn pull_image(&self, reference: &str) -> Result<()> {
        let args = self.protocol.pull_image_args(reference);
        self.exec_raw(&args).await.map(|_| ())
    }

    async fn list_images(&self) -> Result<Vec<ImageInfo>> {
        let args = self.protocol.list_images_args();
        let (stdout, _) = self.exec_raw(&args).await?;
        self.protocol.parse_list_images_output(&stdout)
    }

    async fn remove_image(&self, reference: &str, force: bool) -> Result<()> {
        let args = self.protocol.remove_image_args(reference, force);
        self.exec_raw(&args).await.map(|_| ())
    }

    async fn create_network(&self, name: &str, config: &ComposeNetwork) -> Result<()> {
        let args = self.protocol.create_network_args(name, config);
        self.exec_raw(&args).await.map(|_| ())
    }

    async fn remove_network(&self, name: &str) -> Result<()> {
        let args = self.protocol.remove_network_args(name);
        self.exec_raw(&args).await.map(|_| ())
    }

    async fn create_volume(&self, name: &str, config: &ComposeVolume) -> Result<()> {
        let args = self.protocol.create_volume_args(name, config);
        self.exec_raw(&args).await.map(|_| ())
    }

    async fn remove_volume(&self, name: &str) -> Result<()> {
        let args = self.protocol.remove_volume_args(name);
        self.exec_raw(&args).await.map(|_| ())
    }

    async fn inspect_network(&self, name: &str) -> Result<()> {
        let args = self.protocol.inspect_network_args(name);
        self.exec_raw(&args).await.map(|_| ())
    }

    async fn inspect_volume(&self, name: &str) -> Result<()> {
        let args = self.protocol.inspect_volume_args(name);
        self.exec_raw(&args).await.map(|_| ())
    }

    async fn inspect_image(&self, reference: &str) -> Result<ImageInfo> {
        let args = self.protocol.inspect_image_args(reference);
        let (stdout, _) = self.exec_raw(&args).await?;
        let images = self.protocol.parse_list_images_output(&stdout)?;
        images
            .into_iter()
            .next()
            .ok_or_else(|| ComposeError::NotFound(reference.to_string()))
    }

    async fn build(&self, spec: &ComposeServiceBuild, image_name: &str) -> Result<()> {
        let args = self.protocol.build_args(spec, image_name);
        self.exec_raw(&args).await.map(|_| ())
    }

    async fn run_with_security(
        &self,
        spec: &ContainerSpec,
        profile: &SecurityProfile,
    ) -> Result<ContainerHandle> {
        // Cross-backend determinism pass (see `crate::capabilities`):
        // normalise the spec and security profile against the backend's
        // declared capabilities BEFORE emitting CLI args. Drops fields
        // the backend can't honor + emits structured warnings via
        // tracing so the user can grep for them. This is the layer
        // that prevents an apple/container `run` from receiving a
        // `--privileged` flag the CLI rejects.
        let caps = self.protocol.capabilities();
        let svc_name = spec.name.as_deref().unwrap_or("<unnamed>");
        let mut normalised_spec = spec.clone();
        let mut normalised_profile = profile.clone();
        let mut warnings =
            crate::capabilities::normalise_spec_for(caps, svc_name, &mut normalised_spec);
        warnings.extend(crate::capabilities::normalise_security_profile(
            caps,
            svc_name,
            &mut normalised_profile,
        ));
        for w in &warnings {
            tracing::warn!(
                target: "perry::container::normalise",
                backend = w.backend,
                service = %w.service,
                field = w.field,
                reason = %w.reason,
                "spec field dropped/translated for backend"
            );
        }

        let mut args = self.protocol.run_args(&normalised_spec);
        // Find the image name to insert security args before it
        if let Some(pos) = args.iter().position(|a| a == &normalised_spec.image) {
            let sec_args = self.protocol.security_args(&normalised_profile);
            // If it's lima, we need to be careful with where we insert.
            // But let's assume we can just insert before the image.
            for (i, arg) in sec_args.into_iter().enumerate() {
                args.insert(pos + i, arg);
            }
        }

        let (stdout, _) = self.exec_raw(&args).await?;
        let id = self.protocol.parse_container_id(&stdout)?;
        Ok(ContainerHandle {
            id,
            name: normalised_spec.name,
        })
    }

    async fn wait(&self, id: &str) -> Result<i32> {
        // `docker/podman wait <id>` blocks until the container exits and prints the exit code.
        let output = Command::new(&self.bin)
            .args(["wait", id])
            .output()
            .await
            .map_err(ComposeError::IoError)?;
        let code_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(code_str.parse::<i32>().unwrap_or(-1))
    }
}

pub async fn detect_backend() -> Result<Box<dyn ContainerBackend>> {
    // `PERRY_CONTAINER_BACKEND` accepts EITHER a single name (single-pin)
    // OR a comma-separated list (user-defined priority — try each in
    // order, first available wins). This is the env-var-side of the
    // `setBackends(names: string[])` TS API. Examples:
    //
    //     PERRY_CONTAINER_BACKEND=docker
    //     PERRY_CONTAINER_BACKEND=podman,docker
    //     PERRY_CONTAINER_BACKEND=apple/container,podman,docker
    //
    // Whitespace around commas is tolerated. Empty entries are skipped.
    if let Ok(raw) = std::env::var("PERRY_CONTAINER_BACKEND") {
        let user_priority: Vec<&str> = raw
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        if user_priority.is_empty() {
            // Treat empty / all-whitespace as "ignore the env var" rather
            // than as a hard error — feels less footgun-y for users who
            // do `PERRY_CONTAINER_BACKEND= ./app` to clear it.
        } else {
            let mut results = Vec::new();
            for candidate in &user_priority {
                match tokio::time::timeout(Duration::from_secs(2), probe_candidate(candidate)).await
                {
                    Ok(Ok(backend)) => return Ok(backend),
                    Ok(Err(reason)) => results.push(BackendProbeResult {
                        name: candidate.to_string(),
                        available: false,
                        reason,
                    }),
                    Err(_) => results.push(BackendProbeResult {
                        name: candidate.to_string(),
                        available: false,
                        reason: "probe timed out".into(),
                    }),
                }
            }
            return Err(ComposeError::NoBackendFound { probed: results });
        }
    }

    let candidates = platform_candidates();
    let mut results = Vec::new();

    for candidate in candidates {
        match tokio::time::timeout(Duration::from_secs(2), probe_candidate(candidate)).await {
            Ok(Ok(backend)) => return Ok(backend),
            Ok(Err(reason)) => results.push(BackendProbeResult {
                name: candidate.to_string(),
                available: false,
                reason,
            }),
            Err(_) => results.push(BackendProbeResult {
                name: candidate.to_string(),
                available: false,
                reason: "probe timed out".into(),
            }),
        }
    }

    Err(ComposeError::NoBackendFound { probed: results })
}

/// Probe **every** candidate in `platform_candidates()` and return one
/// `BackendProbeResult` per name, regardless of whether any of them
/// succeed. Unlike `detect_backend()`, this never short-circuits — the
/// result is the full picture of what's installed and reachable on
/// this host, in platform-priority order.
///
/// Use this for diagnostics, BackendInstaller fallback, CI-matrix
/// "which lanes can run on this runner", and TS-side
/// `getAvailableBackends()`. Each candidate gets a 2-second probe
/// timeout (same as `detect_backend()`).
///
/// **Determinism:** the function always probes in the order returned
/// by `platform_candidates()`, which is compile-time-stable per
/// platform. Two calls in quick succession yield the same probe
/// results unless the host's runtime state changes between calls.
pub async fn probe_all_candidates() -> Vec<BackendProbeResult> {
    let candidates = platform_candidates();
    let mut results = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        match tokio::time::timeout(Duration::from_secs(2), probe_candidate(candidate)).await {
            Ok(Ok(_backend)) => results.push(BackendProbeResult {
                name: candidate.to_string(),
                available: true,
                reason: String::new(),
            }),
            Ok(Err(reason)) => results.push(BackendProbeResult {
                name: candidate.to_string(),
                available: false,
                reason,
            }),
            Err(_) => results.push(BackendProbeResult {
                name: candidate.to_string(),
                available: false,
                reason: "probe timed out".into(),
            }),
        }
    }
    results
}

/// Backend probe order for the current platform.
///
/// Encodes three priorities, in descending precedence:
///
/// 1. **Platform-native runtimes win** — `apple/container` on macOS/iOS
///    (the only Apple-native OCI runtime).
/// 2. **Daemonless / OCI-compatible / rootless beat daemon-based** —
///    `podman` (rootless, daemonless, OCI-compatible) ranks ahead of
///    `docker` (root daemon) on every platform.
/// 3. **Docker is always the fallback** — never preferred, never first;
///    chosen only when nothing else is probeable.
///
/// Per-process override via `PERRY_CONTAINER_BACKEND=<name>` env var
/// (precedence over this list — disables auto-detection entirely).
/// Programmatic override via `js_container_setBackend(name)` (TS-side).
pub fn platform_candidates() -> &'static [&'static str] {
    if cfg!(target_os = "macos") || cfg!(target_os = "ios") {
        &[
            "apple/container",
            "orbstack",
            "colima",
            "rancher-desktop",
            "lima",
            "podman",
            "nerdctl",
            "docker",
        ]
    } else if cfg!(target_os = "linux") {
        &["podman", "nerdctl", "docker"]
    } else {
        // Windows and other platforms
        &["podman", "nerdctl", "docker"]
    }
}

async fn probe_candidate(name: &str) -> std::result::Result<Box<dyn ContainerBackend>, String> {
    let which_bin = |name: &str| -> std::result::Result<PathBuf, String> {
        which::which(name).map_err(|_| format!("{} not found", name))
    };

    match name {
        "apple/container" => {
            // Two-step probe: (1) the binary must be on PATH, (2) it must
            // actually respond to a `--version` query (catches the "stale
            // homebrew shim that points at a deleted Cellar dir" case).
            // We do **not** require `container system start` to have
            // succeeded — the orchestrator does still work for image-pull
            // / build / run / list / logs / exec / stop without the
            // network plugin loaded. Only `network create / inspect /
            // delete` will fail, and those produce a clear error message
            // ("Plugin 'container-network' not found") that the engine
            // surfaces unchanged. Forcing system-start at probe time
            // would be a much higher bar than other backends face
            // (Docker doesn't require its daemon at probe time either).
            let bin = which_bin("container")?;
            let out = Command::new(&bin)
                .arg("--version")
                .output()
                .await
                .map_err(|e| format!("apple/container --version failed: {e}"))?;
            if !out.status.success() {
                return Err(format!(
                    "apple/container --version exited {}: {}",
                    out.status.code().unwrap_or(-1),
                    String::from_utf8_lossy(&out.stderr).trim()
                ));
            }
            // Optional sanity log: surface the version in the probe
            // result so users debugging "why is apple/container probe
            // succeeding?" can confirm what was found. Stored in
            // PERRY_CONTAINER_BACKEND_VERSION for diagnostic consumers.
            if let Ok(s) = std::str::from_utf8(&out.stdout) {
                std::env::set_var("PERRY_CONTAINER_BACKEND_VERSION", s.trim());
            }
            Ok(Box::new(CliBackend::new(
                bin,
                Box::new(AppleContainerProtocol),
            )))
        }
        "podman" => {
            let bin = which_bin("podman")?;
            if cfg!(target_os = "macos") {
                let out = Command::new(&bin)
                    .args(&["machine", "list", "--format", "json"])
                    .output()
                    .await
                    .map_err(|_| "podman machine list failed")?;
                let json: serde_json::Value =
                    serde_json::from_slice(&out.stdout).map_err(|_| "invalid podman output")?;
                if !json
                    .as_array()
                    .map(|a| a.iter().any(|m| m["Running"].as_bool().unwrap_or(false)))
                    .unwrap_or(false)
                {
                    return Err("no podman machine running".into());
                }
            }
            Ok(Box::new(CliBackend::new(bin, Box::new(DockerProtocol))))
        }
        "orbstack" => {
            let bin = which_bin("orb")
                .or_else(|_| which_bin("docker"))
                .map_err(|_| "orbstack not found")?;
            Ok(Box::new(CliBackend::new(bin, Box::new(DockerProtocol))))
        }
        "colima" => {
            let bin = which_bin("colima")?;
            let out = Command::new(&bin)
                .arg("status")
                .output()
                .await
                .map_err(|_| "colima status failed")?;
            if !String::from_utf8_lossy(&out.stdout).contains("running") {
                return Err("colima not running".into());
            }
            let dbin = which_bin("docker").map_err(|_| "docker cli not found for colima")?;
            Ok(Box::new(CliBackend::new(dbin, Box::new(DockerProtocol))))
        }
        "lima" => {
            let bin = which_bin("limactl")?;
            let out = Command::new(&bin)
                .args(&["list", "--json"])
                .output()
                .await
                .map_err(|_| "limactl list failed")?;
            let instance = String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
                .find(|v| v["status"] == "Running")
                .and_then(|v| v["name"].as_str().map(|s| s.to_string()))
                .ok_or("no running lima instance")?;
            Ok(Box::new(CliBackend::new(
                bin,
                Box::new(LimaProtocol { instance }),
            )))
        }
        "nerdctl" => {
            let bin = which_bin("nerdctl")?;
            Ok(Box::new(CliBackend::new(bin, Box::new(DockerProtocol))))
        }
        "docker" => {
            let bin = which_bin("docker")?;
            Ok(Box::new(CliBackend::new(bin, Box::new(DockerProtocol))))
        }
        _ => Err("unknown backend".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ContainerSpec;

    #[test]
    fn test_docker_run_args() {
        let proto = DockerProtocol;
        let spec = ContainerSpec {
            image: "nginx".into(),
            name: Some("web".into()),
            ports: Some(vec!["80:80".into()]),
            env: Some([("FOO".into(), "BAR".into())].into()),
            rm: Some(true),
            ..Default::default()
        };

        let args = proto.run_args(&spec);
        assert!(args.contains(&"run".to_string()));
        assert!(args.contains(&"--name".to_string()));
        assert!(args.contains(&"web".to_string()));
        assert!(args.contains(&"-p".to_string()));
        assert!(args.contains(&"80:80".to_string()));
        assert!(args.contains(&"-e".to_string()));
        assert!(args.contains(&"FOO=BAR".to_string()));
        assert!(args.contains(&"--rm".to_string()));
        assert!(args.contains(&"nginx".to_string()));
    }

    #[test]
    fn test_docker_run_args_includes_network_alias() {
        // Service-key network alias regression: pre-fix Perry's compose
        // engine relied on `container_name` for cross-service DNS,
        // breaking any port of a docker-compose stack from the wider
        // ecosystem. The fix populates `network_aliases` from the
        // service KEY in `ComposeEngine::up`; this test pins that
        // `--network-alias <name>` is emitted per entry.
        let proto = DockerProtocol;
        let spec = ContainerSpec {
            image: "postgres:16-alpine".into(),
            name: Some("myapp_db_abc12345".into()),
            network: Some("myapp_appnet".into()),
            network_aliases: Some(vec!["db".into(), "primary-db".into()]),
            ..Default::default()
        };
        let args = proto.run_args(&spec);
        assert!(
            args.windows(2)
                .any(|w| w[0] == "--network-alias" && w[1] == "db"),
            "expected --network-alias db; got {:?}",
            args
        );
        assert!(
            args.windows(2)
                .any(|w| w[0] == "--network-alias" && w[1] == "primary-db"),
            "expected --network-alias primary-db; got {:?}",
            args
        );
    }

    #[test]
    fn test_docker_run_args_emits_seccomp_when_set() {
        let proto = DockerProtocol;
        let spec = ContainerSpec {
            image: "alpine".into(),
            // seccomp lives on SecurityProfile, not ContainerSpec, so
            // run_with_security applies it via security_args. Test the
            // security_args output directly:
            ..Default::default()
        };
        let _ = proto.run_args(&spec); // smoke — no panic on minimal spec
        let security_args = proto.security_args(&SecurityProfile {
            read_only_root: true,
            seccomp: Some("default".into()),
            ..Default::default()
        });
        assert!(
            security_args.iter().any(|s| s.contains("seccomp")),
            "expected seccomp in security args; got {:?}",
            security_args
        );
    }

    #[test]
    fn test_docker_run_args_emits_entrypoint_array_form() {
        let proto = DockerProtocol;
        let spec = ContainerSpec {
            image: "alpine".into(),
            entrypoint: Some(vec!["/usr/bin/env".into(), "sh".into()]),
            ..Default::default()
        };
        let args = proto.run_args(&spec);
        let ep_idx = args
            .iter()
            .position(|s| s == "--entrypoint")
            .expect("expected --entrypoint flag");
        assert!(
            ep_idx + 1 < args.len(),
            "--entrypoint must have a value after it; got {:?}",
            args
        );
    }

    #[test]
    fn test_docker_run_args_omits_rm_when_unset() {
        // Conservative-default invariant: `rm: None` MUST NOT emit
        // `--rm`. Otherwise containers would silently auto-remove on
        // exit, defeating debug-after-failure workflows.
        let proto = DockerProtocol;
        let spec = ContainerSpec {
            image: "alpine".into(),
            rm: None,
            ..Default::default()
        };
        let args = proto.run_args(&spec);
        assert!(
            !args.iter().any(|s| s == "--rm"),
            "rm: None must NOT emit --rm; got {:?}",
            args
        );
    }

    #[test]
    fn test_docker_run_args_omits_optional_flags_when_unset() {
        // Snapshot-style invariant: a minimal spec produces only
        // `run --detach <image>` plus image. No spurious flags.
        let proto = DockerProtocol;
        let spec = ContainerSpec {
            image: "alpine".into(),
            ..Default::default()
        };
        let args = proto.run_args(&spec);
        let unwanted = [
            "--privileged",
            "--read-only",
            "--user",
            "--workdir",
            "--cap-add",
            "--cap-drop",
            "--rm",
            "--name",
            "--network",
        ];
        for flag in unwanted {
            assert!(
                !args.iter().any(|s| s == flag),
                "minimal spec must NOT emit `{flag}`; got {:?}",
                args
            );
        }
    }

    #[test]
    fn test_apple_run_args_emits_detach_for_orchestrator() {
        // apple/container `run` is foreground-by-default. The orchestrator
        // needs the container ID back so it can move on — so `--detach`
        // is required, NOT prohibited. Pre-v0.5.374 the engine called the
        // foreground form and blocked on the container's main process,
        // making compose stacks effectively unworkable on apple/container.
        let proto = AppleContainerProtocol;
        let spec = ContainerSpec {
            image: "alpine".into(),
            ..Default::default()
        };
        let args = proto.run_args(&spec);
        assert!(
            args.iter().any(|s| s == "--detach"),
            "apple/container run MUST include --detach for orchestrator; got {:?}",
            args
        );
    }

    #[test]
    fn test_apple_run_args_includes_network_alias() {
        let proto = AppleContainerProtocol;
        let spec = ContainerSpec {
            image: "alpine".into(),
            network: Some("appnet".into()),
            network_aliases: Some(vec!["worker".into()]),
            ..Default::default()
        };
        let args = proto.run_args(&spec);
        assert!(
            args.windows(2)
                .any(|w| w[0] == "--network-alias" && w[1] == "worker"),
            "apple/container should emit --network-alias too; got {:?}",
            args
        );
    }

    #[test]
    fn test_docker_security_run_args() {
        let proto = DockerProtocol;
        let spec = ContainerSpec {
            image: "nginx".into(),
            privileged: Some(true),
            user: Some("nobody".into()),
            workdir: Some("/tmp".into()),
            cap_add: Some(vec!["NET_ADMIN".into()]),
            cap_drop: Some(vec!["ALL".into()]),
            read_only: Some(true),
            ..Default::default()
        };

        let args = proto.run_args(&spec);
        assert!(args.contains(&"--privileged".to_string()));
        assert!(args.contains(&"--user".to_string()));
        assert!(args.contains(&"nobody".to_string()));
        assert!(args.contains(&"--workdir".to_string()));
        assert!(args.contains(&"/tmp".to_string()));
        assert!(args.contains(&"--cap-add".to_string()));
        assert!(args.contains(&"NET_ADMIN".to_string()));
        assert!(args.contains(&"--cap-drop".to_string()));
        assert!(args.contains(&"ALL".to_string()));
        assert!(args.contains(&"--read-only".to_string()));
    }

    #[test]
    fn test_apple_run_args() {
        let proto = AppleContainerProtocol;
        let spec = ContainerSpec {
            image: "alpine".into(),
            rm: Some(true),
            ..Default::default()
        };

        let args = proto.run_args(&spec);
        assert!(args.contains(&"run".to_string()));
        assert!(args.contains(&"--detach".to_string()));
        assert!(args.contains(&"--rm".to_string()));
        assert!(args.contains(&"alpine".to_string()));
    }

    #[test]
    fn test_apple_run_args_drops_privileged() {
        // apple/container does NOT support `--privileged` (Linux
        // containers run inside an Apple-VM; host-privilege escalation
        // isn't a concept). We must silently drop it from the spec
        // rather than emit a flag the CLI rejects.
        let proto = AppleContainerProtocol;
        let spec = ContainerSpec {
            image: "alpine".into(),
            privileged: Some(true),
            ..Default::default()
        };
        let args = proto.run_args(&spec);
        assert!(
            !args.iter().any(|s| s == "--privileged"),
            "apple/container must NOT emit --privileged; got {:?}",
            args
        );
    }

    #[test]
    fn test_apple_security_args_drops_seccomp() {
        // apple/container has no equivalent of Docker's
        // `--security-opt seccomp=<file>` (the syscall-filter model is
        // VM-host-managed). Honor only `--read-only`; drop seccomp.
        let proto = AppleContainerProtocol;
        let args = proto.security_args(&SecurityProfile {
            read_only_root: true,
            seccomp: Some("default".into()),
            ..Default::default()
        });
        assert!(args.iter().any(|s| s == "--read-only"));
        assert!(
            !args.iter().any(|s| s.contains("seccomp")),
            "apple/container security_args must drop seccomp; got {:?}",
            args
        );
    }

    #[test]
    fn test_apple_logs_uses_n_not_tail() {
        // apple/container's `logs` accepts `-n <N>` (the canonical name);
        // there is no `--tail` long form. Emitting `--tail` produces
        // "unknown flag" from the apple CLI.
        let proto = AppleContainerProtocol;
        let args = proto.logs_args("abc123", Some(50));
        assert_eq!(args[0], "logs");
        assert!(
            args.windows(2).any(|w| w[0] == "-n" && w[1] == "50"),
            "expected `-n 50`; got {:?}",
            args
        );
        assert!(
            !args.iter().any(|s| s == "--tail"),
            "apple/container must NOT emit --tail; got {:?}",
            args
        );
    }

    #[test]
    fn test_apple_list_uses_list_not_ps() {
        // apple/container has `list` / `ls` only — no `ps` alias.
        let proto = AppleContainerProtocol;
        let args = proto.list_args(true);
        assert_eq!(args[0], "list");
        assert!(args.contains(&"--format".to_string()));
        assert!(args.contains(&"json".to_string()));
        assert!(args.contains(&"--all".to_string()));
        assert!(
            !args.iter().any(|s| s == "ps"),
            "apple/container must NOT emit `ps`; got {:?}",
            args
        );
    }

    #[test]
    fn test_apple_inspect_drops_format_flag() {
        // apple/container's `inspect` outputs JSON natively. It does
        // NOT accept `--format` — emitting it produces "unknown flag".
        let proto = AppleContainerProtocol;
        let args = proto.inspect_args("abc123");
        assert_eq!(args[0], "inspect");
        assert!(
            !args.iter().any(|s| s == "--format"),
            "apple/container inspect must NOT emit --format; got {:?}",
            args
        );
    }

    #[test]
    fn test_apple_image_subcommand_routing() {
        // Image ops live under the `image` subcommand on apple/container.
        // Verify pull / list-images / remove-image / inspect-image all
        // route through it.
        let proto = AppleContainerProtocol;

        let pull = proto.pull_image_args("alpine:3.20");
        assert_eq!(&pull[..2], &["image".to_string(), "pull".to_string()]);
        assert_eq!(pull.last().unwrap(), "alpine:3.20");

        let list = proto.list_images_args();
        assert_eq!(&list[..2], &["image".to_string(), "list".to_string()]);
        assert!(list.iter().any(|s| s == "json"));

        let remove = proto.remove_image_args("alpine:3.20", true);
        assert_eq!(&remove[..2], &["image".to_string(), "delete".to_string()]);
        assert!(remove.iter().any(|s| s == "--force"));

        let inspect = proto.inspect_image_args("alpine:3.20");
        assert_eq!(&inspect[..2], &["image".to_string(), "inspect".to_string()]);
        // Inspect must NOT pass --format (apple outputs JSON natively)
        assert!(!inspect.iter().any(|s| s == "--format"));
    }

    #[test]
    fn test_apple_remove_uses_delete_canonical_form() {
        // apple/container's canonical removal is `delete` (with `rm` as
        // alias). Use the canonical name so logs read consistently.
        let proto = AppleContainerProtocol;
        let args = proto.remove_args("abc123", true);
        assert_eq!(args[0], "delete");
        assert!(args.iter().any(|s| s == "--force"));
    }

    #[test]
    fn test_apple_volume_create_drops_driver() {
        // apple/container's `volume create` does NOT accept `--driver`
        // (the volume model is local-only). The spec may carry a driver
        // string from a docker-compose file; we silently drop it.
        let proto = AppleContainerProtocol;
        let cfg = ComposeVolume {
            driver: Some("local".into()),
            ..Default::default()
        };
        let args = proto.create_volume_args("data", &cfg);
        assert_eq!(&args[..2], &["volume".to_string(), "create".to_string()]);
        assert!(
            !args.iter().any(|s| s == "--driver"),
            "apple/container volume create must NOT emit --driver; got {:?}",
            args
        );
        assert_eq!(args.last().unwrap(), "data");
    }

    #[test]
    fn test_apple_volume_remove_uses_delete() {
        let proto = AppleContainerProtocol;
        let args = proto.remove_volume_args("data");
        assert_eq!(args, vec!["volume", "delete", "data"]);
    }

    #[test]
    fn test_apple_network_create_drops_driver() {
        // apple/container's network model doesn't expose docker's
        // `--driver bridge` flag — the driver is implicit in the
        // apple-network plugin.
        let proto = AppleContainerProtocol;
        let cfg = ComposeNetwork {
            driver: Some("bridge".into()),
            ..Default::default()
        };
        let args = proto.create_network_args("appnet", &cfg);
        assert_eq!(&args[..2], &["network".to_string(), "create".to_string()]);
        assert!(
            !args.iter().any(|s| s == "--driver"),
            "apple/container network create must NOT emit --driver; got {:?}",
            args
        );
        assert_eq!(args.last().unwrap(), "appnet");
    }

    #[test]
    fn test_apple_network_remove_uses_delete() {
        let proto = AppleContainerProtocol;
        let args = proto.remove_network_args("appnet");
        assert_eq!(args, vec!["network", "delete", "appnet"]);
    }

    #[test]
    fn test_apple_create_args_no_detach() {
        // `create` has no detach concept — that's `start`'s job.
        let proto = AppleContainerProtocol;
        let spec = ContainerSpec {
            image: "alpine".into(),
            ..Default::default()
        };
        let args = proto.create_args(&spec);
        assert_eq!(args[0], "create");
        assert!(
            !args.iter().any(|s| s == "--detach"),
            "apple/container create must NOT emit --detach; got {:?}",
            args
        );
    }

    #[test]
    fn test_apple_parse_list_output_handles_empty_array() {
        let proto = AppleContainerProtocol;
        let infos = proto.parse_list_output("[]").expect("empty array parses");
        assert!(infos.is_empty());
    }

    #[test]
    fn test_apple_parse_list_output_apple_shape() {
        // Mirrors apple/container 0.12's `list --format json` shape:
        // a JSON array of `{ configuration: { id, image: { reference } },
        // status, networks: [{ address }] }` objects.
        let proto = AppleContainerProtocol;
        let stdout = r#"[
            {
                "configuration": {
                    "id": "abc123def456",
                    "image": { "reference": "docker.io/library/alpine:3.20" },
                    "hostname": "alpine-test",
                    "labels": { "perry.compose.project": "test" }
                },
                "status": "running",
                "networks": [{ "address": "10.0.0.5" }]
            }
        ]"#;
        let infos = proto.parse_list_output(stdout).expect("parse ok");
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].id, "abc123def456");
        assert_eq!(infos[0].name, "alpine-test");
        assert_eq!(infos[0].image, "docker.io/library/alpine:3.20");
        assert_eq!(infos[0].status, "running");
        assert_eq!(infos[0].ip_address, "10.0.0.5");
        assert_eq!(
            infos[0].labels.get("perry.compose.project"),
            Some(&"test".to_string())
        );
    }

    #[test]
    fn test_apple_parse_inspect_output_apple_shape() {
        let proto = AppleContainerProtocol;
        let stdout = r#"[
            {
                "configuration": {
                    "id": "ctr-id",
                    "image": { "reference": "alpine:latest" },
                    "hostname": "ctr-name",
                    "labels": {}
                },
                "status": "running",
                "networks": []
            }
        ]"#;
        let info = proto.parse_inspect_output(stdout).expect("parse ok");
        assert_eq!(info.id, "ctr-id");
        assert_eq!(info.name, "ctr-name");
        assert_eq!(info.image, "alpine:latest");
        assert_eq!(info.status, "running");
        assert_eq!(info.ip_address, "");
    }

    #[test]
    fn test_apple_parse_inspect_output_falls_back_to_docker_shape() {
        // Defensive: some apple-compatible runtimes emit docker-shaped
        // inspect output. The fallback parser should pick those up.
        let proto = AppleContainerProtocol;
        let stdout = r#"[
            {
                "Id": "docker-id",
                "Name": "docker-name",
                "Config": { "Image": "alpine:latest", "Labels": {} },
                "State": { "Status": "running" },
                "Created": "2026-04-28T12:00:00Z",
                "NetworkSettings": { "IPAddress": "172.17.0.2", "Networks": {} }
            }
        ]"#;
        let info = proto.parse_inspect_output(stdout).expect("parse ok");
        assert_eq!(info.id, "docker-id");
        assert_eq!(info.name, "docker-name");
        assert_eq!(info.ip_address, "172.17.0.2");
    }

    #[test]
    fn test_apple_parse_list_images_output_apple_shape() {
        let proto = AppleContainerProtocol;
        let stdout = r#"[
            {
                "reference": "docker.io/library/alpine:3.20",
                "id": "sha256:abc123",
                "size": 7654321,
                "createdAt": "2026-04-01T00:00:00Z"
            },
            {
                "reference": "docker.io/library/postgres:16-alpine",
                "id": "sha256:def456",
                "size": 234567890
            }
        ]"#;
        let images = proto.parse_list_images_output(stdout).expect("parse ok");
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].repository, "docker.io/library/alpine");
        assert_eq!(images[0].tag, "3.20");
        assert_eq!(images[0].id, "sha256:abc123");
        assert_eq!(images[1].repository, "docker.io/library/postgres");
        assert_eq!(images[1].tag, "16-alpine");
    }

    #[test]
    fn test_split_image_reference_handles_registry_port() {
        // Registry hostname with port: `localhost:5000/repo:tag` must NOT
        // split on the registry's `:5000` colon.
        let (repo, tag) = split_image_reference("localhost:5000/repo:1.0");
        assert_eq!(repo, "localhost:5000/repo");
        assert_eq!(tag, "1.0");
    }

    #[test]
    fn test_split_image_reference_handles_digest() {
        let (repo, tag) = split_image_reference("alpine@sha256:abc123def456");
        assert_eq!(repo, "alpine");
        assert_eq!(tag, "sha256:abc123def456");
    }

    #[test]
    fn test_split_image_reference_defaults_to_latest() {
        let (repo, tag) = split_image_reference("alpine");
        assert_eq!(repo, "alpine");
        assert_eq!(tag, "latest");
    }

    #[test]
    fn test_apple_run_args_includes_labels() {
        // The compose engine writes `perry.compose.project` and
        // `perry.compose.spec_hash` labels on every container; these
        // drive `downByProject` cleanup and spec-drift detection. Pin
        // that apple emits them.
        let proto = AppleContainerProtocol;
        let mut labels = HashMap::new();
        labels.insert("perry.compose.project".into(), "myproj".into());
        labels.insert("perry.compose.spec_hash".into(), "abcd1234".into());
        let spec = ContainerSpec {
            image: "alpine".into(),
            labels: Some(labels),
            ..Default::default()
        };
        let args = proto.run_args(&spec);
        let label_pairs: Vec<&str> = args
            .windows(2)
            .filter(|w| w[0] == "--label")
            .map(|w| w[1].as_str())
            .collect();
        assert!(
            label_pairs
                .iter()
                .any(|s| *s == "perry.compose.project=myproj"),
            "expected project label; got {:?}",
            label_pairs
        );
        assert!(
            label_pairs
                .iter()
                .any(|s| *s == "perry.compose.spec_hash=abcd1234"),
            "expected spec_hash label; got {:?}",
            label_pairs
        );
    }

    #[test]
    fn test_lima_run_args() {
        let proto = LimaProtocol {
            instance: "default".into(),
        };
        let spec = ContainerSpec {
            image: "busybox".into(),
            ..Default::default()
        };

        let args = proto.run_args(&spec);
        assert_eq!(args[0], "shell");
        assert_eq!(args[1], "default");
        assert_eq!(args[2], "nerdctl");
        assert_eq!(args[3], "run");
    }

    #[test]
    fn test_platform_candidates() {
        let candidates = platform_candidates();
        assert!(!candidates.is_empty());
        if cfg!(target_os = "macos") || cfg!(target_os = "ios") {
            assert_eq!(candidates[0], "apple/container");
        } else {
            assert_eq!(candidates[0], "podman");
        }
    }

    /// All env-var-mutating tests in one function. cargo runs tests
    /// in parallel by default and `std::env::set_var` is process-global,
    /// so independent `#[tokio::test]` cases would race the env var
    /// across threads and produce flaky results. Consolidate sequentially
    /// rather than depend on a serial-test crate (avoids the dep + the
    /// per-test setup overhead of `#[serial]`).
    #[tokio::test]
    async fn test_detect_backend_env_override_behavior() {
        // -------------------------------------------------------------
        // Phase 1: single name (existing behavior, backwards-compat)
        // -------------------------------------------------------------
        std::env::set_var("PERRY_CONTAINER_BACKEND", "invalid-backend-name");
        let res = detect_backend().await;
        std::env::remove_var("PERRY_CONTAINER_BACKEND");

        if let Err(ComposeError::NoBackendFound { probed }) = res {
            assert_eq!(probed.len(), 1);
            assert_eq!(probed[0].name, "invalid-backend-name");
            assert_eq!(probed[0].reason, "unknown backend");
        } else {
            panic!("Expected NoBackendFound error from single-name override");
        }

        // -------------------------------------------------------------
        // Phase 2: comma-separated user priority list (v0.5.380 feature)
        // -------------------------------------------------------------
        // Each name in the list gets probed in order. All-invalid case:
        // returns NoBackendFound with one BackendProbeResult per
        // attempted name, order preserved.
        std::env::set_var("PERRY_CONTAINER_BACKEND", "bogus-one,bogus-two,bogus-three");
        let res = detect_backend().await;
        std::env::remove_var("PERRY_CONTAINER_BACKEND");

        if let Err(ComposeError::NoBackendFound { probed }) = res {
            assert_eq!(probed.len(), 3, "expected one probe per name");
            assert_eq!(probed[0].name, "bogus-one");
            assert_eq!(probed[1].name, "bogus-two");
            assert_eq!(probed[2].name, "bogus-three");
            assert!(probed.iter().all(|p| p.reason.contains("unknown")));
        } else {
            panic!("Expected NoBackendFound error from comma-separated list");
        }

        // -------------------------------------------------------------
        // Phase 3: tolerant parsing — whitespace + empty entries
        // -------------------------------------------------------------
        // Real env-var input `"a, b,,c"` shouldn't produce 4 probe
        // entries. Trim each entry; skip empties.
        std::env::set_var("PERRY_CONTAINER_BACKEND", "  bogus-a  , bogus-b ,, ");
        let res = detect_backend().await;
        std::env::remove_var("PERRY_CONTAINER_BACKEND");

        if let Err(ComposeError::NoBackendFound { probed }) = res {
            assert_eq!(probed.len(), 2);
            assert_eq!(probed[0].name, "bogus-a");
            assert_eq!(probed[1].name, "bogus-b");
        } else {
            panic!("Expected NoBackendFound error from whitespace-padded list");
        }

        // -------------------------------------------------------------
        // Phase 4: empty string falls through to platform default
        // -------------------------------------------------------------
        // `PERRY_CONTAINER_BACKEND= ./app` is a real shell idiom for
        // "clear an override inherited from the parent env." It
        // shouldn't error; should behave as if the var was unset.
        std::env::set_var("PERRY_CONTAINER_BACKEND", "");
        let res = detect_backend().await;
        std::env::remove_var("PERRY_CONTAINER_BACKEND");

        // Can't assert Ok vs Err deterministically (depends on test
        // runner's installed runtimes), but if Err, the probed list
        // length must match platform_candidates, NOT 0 (which would
        // mean the empty-list path was taken).
        if let Err(ComposeError::NoBackendFound { probed }) = res {
            let candidates = platform_candidates();
            assert_eq!(
                probed.len(),
                candidates.len(),
                "empty env var should fall through to platform_candidates probe"
            );
        }
    }
}
