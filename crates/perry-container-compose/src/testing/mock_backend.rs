use crate::backend::{ContainerBackend, SecurityProfile};
use crate::error::{ComposeError, Result};
use crate::types::{
    ComposeNetwork, ComposeServiceBuild, ComposeVolume, ContainerHandle, ContainerInfo,
    ContainerLogs, ContainerSpec, ImageInfo,
};
use async_trait::async_trait;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

/// Inspect-call response mode for [`MockBackend`].
///
/// The orchestrator decides between `start_command` / `run_command` /
/// `build_command` paths based on whether `backend.inspect()` returns
/// `Ok(running)`, `Ok(stopped)`, or `Err(NotFound)`. This enum lets a test
/// pin the mock to a specific path without having to script every call.
#[derive(Debug, Clone, Default)]
pub enum InspectMode {
    /// Default: every inspect returns a "running" container.
    #[default]
    Running,
    /// Every inspect returns a "stopped" container (orchestrator → start).
    Stopped,
    /// Every inspect fails with `ComposeError::NotFound` (orchestrator → run).
    NotFound,
}

#[derive(Debug, Clone)]
pub enum RecordedCall {
    Run(ContainerSpec),
    Create(ContainerSpec),
    Start(String),
    Stop(String, Option<u32>),
    Remove(String, bool),
    List(bool),
    Inspect(String),
    Logs(String, Option<u32>),
    Exec(String, Vec<String>),
    Build(String),
    CreateNetwork(String),
    RemoveNetwork(String),
    CreateVolume(String),
    RemoveVolume(String),
    Wait(String),
}

pub struct MockBackend {
    pub name: String,
    pub calls: Arc<Mutex<Vec<RecordedCall>>>,
    pub responses: Arc<Mutex<VecDeque<Result<serde_json::Value>>>>,
    inspect_mode: Arc<Mutex<InspectMode>>,
    /// When set, `run_with_security` returns Err on the Nth call.
    /// `None` means succeed every time. Counter reset on `script_run_failure_after`.
    run_failure_at: Arc<Mutex<Option<usize>>>,
    run_call_count: Arc<Mutex<usize>>,
    /// When set, `inspect()` returns this label set on the
    /// `perry.compose.spec_hash` key. `None` means use the legacy
    /// "no spec_hash label" shape (which is what pre-v0.5.372
    /// containers had).
    inspect_spec_hash: Arc<Mutex<Option<String>>>,
}

impl MockBackend {
    /// Construct a named mock with default `InspectMode::Running`.
    pub fn named(name: &str) -> Self {
        Self {
            name: name.to_string(),
            calls: Arc::new(Mutex::new(Vec::new())),
            responses: Arc::new(Mutex::new(VecDeque::new())),
            inspect_mode: Arc::new(Mutex::new(InspectMode::default())),
            run_failure_at: Arc::new(Mutex::new(None)),
            run_call_count: Arc::new(Mutex::new(0)),
            inspect_spec_hash: Arc::new(Mutex::new(None)),
        }
    }

    /// Construct an unnamed mock (uses "mock" as `backend_name`).
    pub fn new() -> Self {
        Self::named("mock")
    }

    pub fn push_ok<T: serde::Serialize>(&self, val: T) {
        self.responses
            .lock()
            .unwrap()
            .push_back(Ok(serde_json::to_value(val).unwrap()));
    }

    pub fn recorded_calls(&self) -> Vec<RecordedCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Async-friendly alias for `recorded_calls()` (matches the test code
    /// style used by `orchestrate::tests`).
    pub async fn calls(&self) -> Vec<RecordedCall> {
        self.recorded_calls()
    }

    /// Force `inspect()` to return either a running or stopped
    /// `ContainerInfo` (`true` → running, `false` → stopped).
    pub async fn set_inspect_running(&self, running: bool) {
        *self.inspect_mode.lock().unwrap() = if running {
            InspectMode::Running
        } else {
            InspectMode::Stopped
        };
    }

    /// Force `inspect()` to return `Err(ComposeError::NotFound)`.
    pub async fn set_inspect_not_found(&self) {
        *self.inspect_mode.lock().unwrap() = InspectMode::NotFound;
    }

    /// Script `run_with_security` to fail on the Nth invocation
    /// (1-indexed). Used by the rollback partial-failure tests.
    /// Earlier invocations succeed normally.
    pub async fn script_run_failure_after(&self, nth_call: usize) {
        *self.run_failure_at.lock().unwrap() = Some(nth_call);
        *self.run_call_count.lock().unwrap() = 0;
    }

    /// Make `inspect()` report a `perry.compose.spec_hash` label that
    /// matches a freshly-computed hash for the given service. Used by
    /// the spec-drift tests' "match" path — when the engine compares
    /// the live label to the freshly-computed value, they match and
    /// the engine takes the skip-because-running path.
    pub async fn set_existing_spec_hash_match(&self, svc: &crate::types::ComposeService) {
        use md5::{Digest, Md5};
        let json = serde_json::to_string(svc).unwrap_or_default();
        let mut h = Md5::new();
        h.update(json.as_bytes());
        let bytes = h.finalize();
        let hash = hex::encode(&bytes[..8]);
        *self.inspect_spec_hash.lock().unwrap() = Some(hash);
    }

    /// Make `inspect()` report a `spec_hash` label with a stale value
    /// (different from any currently-deployed spec). Used by the
    /// spec-drift tests' "drift" path — engine sees the mismatch and
    /// triggers recreate.
    pub async fn set_existing_spec_hash_old(&self) {
        *self.inspect_spec_hash.lock().unwrap() = Some("stale-spec-hash".to_string());
    }
}

impl Default for MockBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ContainerBackend for MockBackend {
    fn backend_name(&self) -> &str {
        &self.name
    }
    async fn check_available(&self) -> Result<()> {
        Ok(())
    }

    async fn build(&self, _spec: &ComposeServiceBuild, image_name: &str) -> Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push(RecordedCall::Build(image_name.to_string()));
        Ok(())
    }

    async fn run(&self, spec: &ContainerSpec) -> Result<ContainerHandle> {
        self.calls
            .lock()
            .unwrap()
            .push(RecordedCall::Run(spec.clone()));
        Ok(ContainerHandle {
            id: format!("mock-{}", spec.name.as_deref().unwrap_or("id")),
            name: spec.name.clone(),
        })
    }

    async fn create(&self, spec: &ContainerSpec) -> Result<ContainerHandle> {
        self.calls
            .lock()
            .unwrap()
            .push(RecordedCall::Create(spec.clone()));
        Ok(ContainerHandle {
            id: format!("mock-{}", spec.name.as_deref().unwrap_or("id")),
            name: spec.name.clone(),
        })
    }

    async fn start(&self, id: &str) -> Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push(RecordedCall::Start(id.to_string()));
        Ok(())
    }

    async fn stop(&self, id: &str, timeout: Option<u32>) -> Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push(RecordedCall::Stop(id.to_string(), timeout));
        Ok(())
    }

    async fn remove(&self, id: &str, force: bool) -> Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push(RecordedCall::Remove(id.to_string(), force));
        Ok(())
    }

    async fn list(&self, all: bool) -> Result<Vec<ContainerInfo>> {
        self.calls.lock().unwrap().push(RecordedCall::List(all));
        Ok(Vec::new())
    }

    async fn inspect(&self, id: &str) -> Result<ContainerInfo> {
        self.calls
            .lock()
            .unwrap()
            .push(RecordedCall::Inspect(id.to_string()));
        let mode = self.inspect_mode.lock().unwrap().clone();
        match mode {
            InspectMode::NotFound => Err(ComposeError::NotFound(id.to_string())),
            InspectMode::Running | InspectMode::Stopped => {
                // Inject the configured `perry.compose.spec_hash` label
                // so the spec-drift detection path in
                // `ComposeEngine::up` can be exercised hermetically.
                let mut labels = HashMap::new();
                if let Some(hash) = self.inspect_spec_hash.lock().unwrap().clone() {
                    labels.insert("perry.compose.spec_hash".into(), hash);
                }
                Ok(ContainerInfo {
                    id: id.to_string(),
                    name: id.to_string(),
                    image: "mock-image".to_string(),
                    status: if matches!(mode, InspectMode::Running) {
                        "running".to_string()
                    } else {
                        "exited".to_string()
                    },
                    ports: Vec::new(),
                    labels,
                    created: "2024-01-01T00:00:00Z".to_string(),
                    ip_address: "172.17.0.2".to_string(),
                })
            }
        }
    }

    async fn inspect_image(&self, reference: &str) -> Result<ImageInfo> {
        Ok(ImageInfo {
            id: "mock-image-id".to_string(),
            repository: reference.to_string(),
            tag: "latest".to_string(),
            size: 0,
            created: "2024-01-01T00:00:00Z".to_string(),
        })
    }

    async fn logs(&self, id: &str, tail: Option<u32>) -> Result<ContainerLogs> {
        self.calls
            .lock()
            .unwrap()
            .push(RecordedCall::Logs(id.to_string(), tail));
        Ok(ContainerLogs {
            stdout: String::new(),
            stderr: String::new(),
        })
    }

    async fn wait(&self, id: &str) -> Result<i32> {
        self.calls
            .lock()
            .unwrap()
            .push(RecordedCall::Wait(id.to_string()));
        Ok(0)
    }

    async fn exec(
        &self,
        id: &str,
        cmd: &[String],
        _env: Option<&HashMap<String, String>>,
        _workdir: Option<&str>,
    ) -> Result<ContainerLogs> {
        self.calls
            .lock()
            .unwrap()
            .push(RecordedCall::Exec(id.to_string(), cmd.to_vec()));
        Ok(ContainerLogs {
            stdout: String::new(),
            stderr: String::new(),
        })
    }

    async fn pull_image(&self, _reference: &str) -> Result<()> {
        Ok(())
    }
    async fn list_images(&self) -> Result<Vec<ImageInfo>> {
        Ok(Vec::new())
    }
    async fn remove_image(&self, _reference: &str, _force: bool) -> Result<()> {
        Ok(())
    }

    async fn create_network(&self, name: &str, _config: &ComposeNetwork) -> Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push(RecordedCall::CreateNetwork(name.to_string()));
        Ok(())
    }

    async fn remove_network(&self, name: &str) -> Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push(RecordedCall::RemoveNetwork(name.to_string()));
        Ok(())
    }

    async fn create_volume(&self, name: &str, _config: &ComposeVolume) -> Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push(RecordedCall::CreateVolume(name.to_string()));
        Ok(())
    }

    async fn remove_volume(&self, name: &str) -> Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push(RecordedCall::RemoveVolume(name.to_string()));
        Ok(())
    }

    async fn inspect_network(&self, name: &str) -> Result<()> {
        // Honor the same InspectMode toggle as `inspect()` so tests
        // can distinguish "network/volume already exists" (engine
        // skips create) from "doesn't exist yet" (engine creates).
        match *self.inspect_mode.lock().unwrap() {
            InspectMode::NotFound => Err(ComposeError::NotFound(name.to_string())),
            _ => Ok(()),
        }
    }
    async fn inspect_volume(&self, name: &str) -> Result<()> {
        match *self.inspect_mode.lock().unwrap() {
            InspectMode::NotFound => Err(ComposeError::NotFound(name.to_string())),
            _ => Ok(()),
        }
    }

    async fn run_with_security(
        &self,
        spec: &ContainerSpec,
        _profile: &SecurityProfile,
    ) -> Result<ContainerHandle> {
        // Honor the scripted-failure-after-N-calls behavior so the
        // rollback partial-failure tests can exercise the failure
        // path without needing a real backend. Read both the target
        // and the new count in a single block so MutexGuards don't
        // span the .await below (would make the future !Send).
        let (target, n) = {
            let target = *self.run_failure_at.lock().unwrap();
            let mut count = self.run_call_count.lock().unwrap();
            *count += 1;
            (target, *count)
        };
        if let Some(t) = target {
            if n == t {
                return Err(ComposeError::BackendError {
                    code: 125,
                    message: format!("scripted run failure on call #{}", n),
                });
            }
        }
        self.run(spec).await
    }

    async fn create_with_security(
        &self,
        spec: &ContainerSpec,
        _profile: &SecurityProfile,
    ) -> Result<ContainerHandle> {
        self.create(spec).await
    }
}
