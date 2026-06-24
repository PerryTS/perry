use super::*;
use crate::error::{ComposeError, Result};
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;

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
                    .args(["machine", "list", "--format", "json"])
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
                .args(["list", "--json"])
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
