use super::*;
use crate::error::{ComposeError, Result};
use crate::types::{
    ComposeNetwork, ComposeServiceBuild, ComposeVolume, ContainerHandle, ContainerInfo,
    ContainerLogs, ContainerSpec, ImageInfo,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;

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

    /// Shared implementation behind `run_with_security` /
    /// `create_with_security`.
    ///
    /// Cross-backend determinism pass (see `crate::capabilities`):
    /// normalise the spec and security profile against the backend's
    /// declared capabilities BEFORE emitting CLI args. Drops fields
    /// the backend can't honor + emits structured warnings via
    /// tracing so the user can grep for them. This is the layer
    /// that prevents an apple/container `run` from receiving a
    /// `--privileged` flag the CLI rejects.
    async fn launch_with_security(
        &self,
        spec: &ContainerSpec,
        profile: &SecurityProfile,
        create_only: bool,
    ) -> Result<ContainerHandle> {
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

        let args = build_secured_args(
            self.protocol.as_ref(),
            &normalised_spec,
            &normalised_profile,
            create_only,
        );

        let (stdout, _) = self.exec_raw(&args).await?;
        let id = self.protocol.parse_container_id(&stdout)?;
        Ok(ContainerHandle {
            id,
            name: normalised_spec.name,
        })
    }
}

/// Insert the protocol's `security_args` immediately before the image
/// Unique stand-in for the image reference while the argv is built, so
/// the image slot is found by construction rather than by matching a
/// value that an option (e.g. `--name`) may legitimately repeat. The
/// control characters keep it distinct from any real image reference.
pub(crate) const IMAGE_SENTINEL: &str = "\u{1}perry-image-sentinel\u{1}";

/// Build the final `run`/`create` argv with the protocol's security
/// flags spliced immediately before the image reference.
///
/// The image slot is located by *construction* rather than by value:
/// the argv is built from a probe spec carrying [`IMAGE_SENTINEL`], so
/// an option value that happens to equal the real image reference
/// (`run --name alpine alpine`) cannot be mistaken for the image and
/// have the flags spliced between a flag and its value. The protocols
/// only ever emit `spec.image` at the image position, so substituting
/// the sentinel back afterwards is exact.
///
/// Pure (the protocol's arg builders are sync), so the whole secured
/// argv shape is unit-testable without spawning a CLI process.
pub(crate) fn build_secured_args(
    protocol: &dyn CliProtocol,
    spec: &ContainerSpec,
    profile: &SecurityProfile,
    create_only: bool,
) -> Vec<String> {
    let mut probe_spec = spec.clone();
    probe_spec.image = IMAGE_SENTINEL.to_string();
    let base_args = if create_only {
        protocol.create_args(&probe_spec)
    } else {
        protocol.run_args(&probe_spec)
    };
    let sec_args = protocol.security_args(profile);
    splice_security_args(base_args, IMAGE_SENTINEL, sec_args)
        .into_iter()
        .map(|a| {
            if a == IMAGE_SENTINEL {
                spec.image.clone()
            } else {
                a
            }
        })
        .collect()
}

/// Insert the protocol's `security_args` immediately before `image` in a
/// `run`/`create` argument vector. The image is the first positional
/// argument; everything after it is the container command, so flags must
/// land before it. Pure function so the final argv shape is
/// unit-testable without spawning a CLI process (see the
/// arg-construction tests in `crate::backend::tests`).
///
/// `image` must occur exactly once, at the image position — production
/// callers pass [`IMAGE_SENTINEL`] and substitute the real reference
/// afterwards, because matching the real reference can hit an earlier
/// option value first (`run --name alpine alpine` would splice between
/// `--name` and its value).
///
/// If the image can't be located (defensive — `run_args` always pushes
/// it) the args are returned unchanged rather than emitting flags into
/// the container command position.
pub(crate) fn splice_security_args(
    mut args: Vec<String>,
    image: &str,
    sec_args: Vec<String>,
) -> Vec<String> {
    if let Some(pos) = args.iter().position(|a| a == image) {
        for (i, arg) in sec_args.into_iter().enumerate() {
            args.insert(pos + i, arg);
        }
    }
    args
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
        self.launch_with_security(spec, profile, false).await
    }

    async fn create_with_security(
        &self,
        spec: &ContainerSpec,
        profile: &SecurityProfile,
    ) -> Result<ContainerHandle> {
        self.launch_with_security(spec, profile, true).await
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
