//! Tests for the v0.5.380 `exec_raw` timeout — pre-fix every CLI call
//! could hang forever if the daemon was wedged. Pinning the timeout
//! behavior so a future refactor that strips the `tokio::time::timeout`
//! wrapper trips a CI failure.
//!
//! These tests use a real binary (`/bin/sleep` on Unix) rather than a
//! mock so they exercise the actual `Command::new(...).output().await`
//! path. The timeout is set via `PERRY_CONTAINER_OP_TIMEOUT_SECS=1` and
//! the sleep duration is longer, guaranteeing the timeout arm fires.

#![cfg(unix)]

use perry_container_compose::backend::{
    CliBackend, CliProtocol, ContainerBackend, SecurityProfile,
};
use perry_container_compose::error::Result;
use perry_container_compose::types::{
    ComposeNetwork, ComposeServiceBuild, ComposeVolume, ContainerInfo, ContainerSpec, ImageInfo,
};
use std::collections::HashMap;
use std::path::PathBuf;

/// A minimal protocol whose every method produces a hand-crafted arg
/// vector — used here so we can run real commands like `sleep 30`
/// without the protocol injecting subcommand prefixes ("pull" / "start"
/// / etc.) that real CLIs need but `/bin/sleep` doesn't recognize.
struct PassthroughProtocol {
    args: Vec<String>,
}

impl CliProtocol for PassthroughProtocol {
    fn run_args(&self, _: &ContainerSpec) -> Vec<String> {
        self.args.clone()
    }
    fn create_args(&self, _: &ContainerSpec) -> Vec<String> {
        self.args.clone()
    }
    fn start_args(&self, _: &str) -> Vec<String> {
        self.args.clone()
    }
    fn stop_args(&self, _: &str, _: Option<u32>) -> Vec<String> {
        self.args.clone()
    }
    fn remove_args(&self, _: &str, _: bool) -> Vec<String> {
        self.args.clone()
    }
    fn list_args(&self, _: bool) -> Vec<String> {
        self.args.clone()
    }
    fn inspect_args(&self, _: &str) -> Vec<String> {
        self.args.clone()
    }
    fn logs_args(&self, _: &str, _: Option<u32>) -> Vec<String> {
        self.args.clone()
    }
    fn exec_args(
        &self,
        _: &str,
        _: &[String],
        _: Option<&HashMap<String, String>>,
        _: Option<&str>,
    ) -> Vec<String> {
        self.args.clone()
    }
    fn pull_image_args(&self, _: &str) -> Vec<String> {
        self.args.clone()
    }
    fn list_images_args(&self) -> Vec<String> {
        self.args.clone()
    }
    fn remove_image_args(&self, _: &str, _: bool) -> Vec<String> {
        self.args.clone()
    }
    fn create_network_args(&self, _: &str, _: &ComposeNetwork) -> Vec<String> {
        self.args.clone()
    }
    fn remove_network_args(&self, _: &str) -> Vec<String> {
        self.args.clone()
    }
    fn create_volume_args(&self, _: &str, _: &ComposeVolume) -> Vec<String> {
        self.args.clone()
    }
    fn remove_volume_args(&self, _: &str) -> Vec<String> {
        self.args.clone()
    }
    fn inspect_network_args(&self, _: &str) -> Vec<String> {
        self.args.clone()
    }
    fn inspect_volume_args(&self, _: &str) -> Vec<String> {
        self.args.clone()
    }
    fn inspect_image_args(&self, _: &str) -> Vec<String> {
        self.args.clone()
    }
    fn build_args(&self, _: &ComposeServiceBuild, _: &str) -> Vec<String> {
        self.args.clone()
    }
    fn security_args(&self, _: &SecurityProfile) -> Vec<String> {
        Vec::new()
    }

    fn parse_list_output(&self, _: &str) -> Result<Vec<ContainerInfo>> {
        Ok(vec![])
    }
    fn parse_inspect_output(&self, _: &str) -> Result<ContainerInfo> {
        Ok(ContainerInfo {
            id: String::new(),
            name: String::new(),
            image: String::new(),
            status: String::new(),
            ports: Vec::new(),
            labels: HashMap::new(),
            created: String::new(),
            ip_address: String::new(),
        })
    }
    fn parse_list_images_output(&self, _: &str) -> Result<Vec<ImageInfo>> {
        Ok(vec![])
    }
    fn parse_container_id(&self, stdout: &str) -> Result<String> {
        Ok(stdout.trim().to_string())
    }
}

// All tests in this file mutate the process-wide
// `PERRY_CONTAINER_OP_TIMEOUT_SECS` env var. cargo runs tests in
// parallel by default, which would race the env var across threads
// and break timing assumptions. Consolidate into one sequential test
// rather than depend on a serial-test crate (avoids the dep + the
// per-test setup cost of `serial_test::serial` macro overhead).
#[tokio::test]
async fn exec_raw_timeout_behavior() {
    let bin = PathBuf::from("/bin/sleep");
    if !bin.exists() {
        eprintln!("skip: /bin/sleep not present on this runner");
        return;
    }

    // Phase 1: timeout fires when command hangs.
    std::env::set_var("PERRY_CONTAINER_OP_TIMEOUT_SECS", "1");
    let proto = PassthroughProtocol {
        args: vec!["30".into()],
    };
    let backend = CliBackend::new(bin.clone(), Box::new(proto));
    let started = std::time::Instant::now();
    let result = backend.pull_image("ignored").await;
    let elapsed = started.elapsed();
    assert!(result.is_err(), "expected timeout error in {:?}", elapsed);
    assert!(
        elapsed < std::time::Duration::from_secs(4),
        "timeout did not fire promptly — took {:?}",
        elapsed
    );
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("hung")
            || err_msg.contains("timeout")
            || err_msg.contains("PERRY_CONTAINER_OP_TIMEOUT_SECS"),
        "timeout error message should explain the timeout + the env var; got: {}",
        err_msg
    );

    // Phase 2: timeout does NOT fire for fast commands.
    std::env::set_var("PERRY_CONTAINER_OP_TIMEOUT_SECS", "10");
    let proto = PassthroughProtocol {
        args: vec!["0".into()],
    };
    let backend = CliBackend::new(bin, Box::new(proto));
    let result = backend.pull_image("ignored").await;
    assert!(
        result.is_ok(),
        "fast command must succeed within timeout; got {:?}",
        result
    );

    std::env::remove_var("PERRY_CONTAINER_OP_TIMEOUT_SECS");
}

#[tokio::test]
async fn exec_raw_truncates_long_stderr_in_error_message() {
    // Pre-fix a multi-MB image-pull failure log ended up verbatim in
    // Error.message. Now `exec_raw` truncates at 4 KiB. Generate a
    // long-stderr failure via /usr/bin/yes (writes "y\n" forever) +
    // exit nonzero (use a pipefail trick via /bin/sh -c).
    //
    // We use /bin/sh -c "yes Y | head -c 100000 1>&2; exit 1"
    // → produces 100 KB of stderr then exits 1. The error message
    // should contain "[truncated, ...]".
    let bin = PathBuf::from("/bin/sh");
    if !bin.exists() {
        return;
    }
    let proto = PassthroughProtocol {
        args: vec!["-c".into(), "yes Y | head -c 100000 1>&2; exit 1".into()],
    };
    let backend = CliBackend::new(bin, Box::new(proto));
    let result = backend.pull_image("ignored").await;
    assert!(result.is_err());
    let msg = format!("{}", result.unwrap_err());
    assert!(
        msg.contains("[truncated"),
        "long stderr must be truncated in error message; got msg of len {}",
        msg.len()
    );
    // Sanity: total error message must be much shorter than 100 KB.
    assert!(
        msg.len() < 10_000,
        "truncation didn't actually shorten the message; len={}",
        msg.len()
    );
}
