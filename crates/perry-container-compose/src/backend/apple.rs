use super::*;
use crate::error::{ComposeError, Result};
use crate::types::{
    ComposeNetwork, ComposeServiceBuild, ComposeVolume, ContainerInfo, ContainerSpec, ImageInfo,
};
use serde::Deserialize;
use std::collections::HashMap;

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
pub(crate) fn split_image_reference(reference: &str) -> (String, String) {
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
