use super::*;
use crate::backend::SecurityProfile;
use crate::testing::mock_backend::{MockBackend, RecordedCall};
use crate::types::{
    ComposeNetwork, ComposeServiceBuild, ComposeVolume, ContainerHandle, ImageInfo,
};
use async_trait::async_trait;

struct ListingBackend {
    inner: MockBackend,
    containers: Vec<ContainerInfo>,
    fail_list: std::sync::atomic::AtomicBool,
    arrivals: Mutex<Vec<ContainerInfo>>,
    arrive_on_stop: Mutex<Vec<ContainerInfo>>,
}

#[async_trait]
impl ContainerBackend for ListingBackend {
    fn backend_name(&self) -> &str {
        "mock"
    }

    async fn list(&self, all: bool) -> Result<Vec<ContainerInfo>> {
        self.inner
            .calls
            .lock()
            .unwrap()
            .push(RecordedCall::List(all));
        if self.fail_list.load(Ordering::Relaxed) {
            return Err(ComposeError::NotFound("list failed".into()));
        }
        let mut snapshot = self.containers.clone();
        snapshot.extend(self.arrivals.lock().unwrap().iter().cloned());
        Ok(snapshot)
    }

    async fn check_available(&self) -> Result<()> {
        self.inner.check_available().await
    }

    async fn build(&self, _spec: &ComposeServiceBuild, image_name: &str) -> Result<()> {
        self.inner.build(_spec, image_name).await
    }

    async fn run(&self, spec: &ContainerSpec) -> Result<ContainerHandle> {
        self.inner.run(spec).await
    }

    async fn create(&self, spec: &ContainerSpec) -> Result<ContainerHandle> {
        self.inner.create(spec).await
    }

    async fn start(&self, id: &str) -> Result<()> {
        self.inner.start(id).await
    }

    async fn stop(&self, id: &str, timeout: Option<u32>) -> Result<()> {
        self.arrivals
            .lock()
            .unwrap()
            .extend(self.arrive_on_stop.lock().unwrap().drain(..));
        self.inner.stop(id, timeout).await
    }

    async fn remove(&self, id: &str, force: bool) -> Result<()> {
        self.inner.remove(id, force).await
    }

    async fn inspect(&self, id: &str) -> Result<ContainerInfo> {
        self.inner.inspect(id).await
    }

    async fn inspect_image(&self, reference: &str) -> Result<ImageInfo> {
        self.inner.inspect_image(reference).await
    }

    async fn logs(&self, id: &str, tail: Option<u32>) -> Result<ContainerLogs> {
        self.inner.logs(id, tail).await
    }

    async fn wait(&self, id: &str) -> Result<i32> {
        self.inner.wait(id).await
    }

    async fn exec(
        &self,
        id: &str,
        cmd: &[String],
        _env: Option<&HashMap<String, String>>,
        _workdir: Option<&str>,
    ) -> Result<ContainerLogs> {
        self.inner.exec(id, cmd, _env, _workdir).await
    }

    async fn pull_image(&self, _reference: &str) -> Result<()> {
        self.inner.pull_image(_reference).await
    }

    async fn list_images(&self) -> Result<Vec<ImageInfo>> {
        self.inner.list_images().await
    }

    async fn remove_image(&self, _reference: &str, _force: bool) -> Result<()> {
        self.inner.remove_image(_reference, _force).await
    }

    async fn create_network(&self, name: &str, _config: &ComposeNetwork) -> Result<()> {
        self.inner.create_network(name, _config).await
    }

    async fn remove_network(&self, name: &str) -> Result<()> {
        self.inner.remove_network(name).await
    }

    async fn create_volume(&self, name: &str, _config: &ComposeVolume) -> Result<()> {
        self.inner.create_volume(name, _config).await
    }

    async fn remove_volume(&self, name: &str) -> Result<()> {
        self.inner.remove_volume(name).await
    }

    async fn inspect_network(&self, name: &str) -> Result<()> {
        self.inner.inspect_network(name).await
    }

    async fn inspect_volume(&self, name: &str) -> Result<()> {
        self.inner.inspect_volume(name).await
    }

    async fn run_with_security(
        &self,
        spec: &ContainerSpec,
        _profile: &SecurityProfile,
    ) -> Result<ContainerHandle> {
        self.inner.run_with_security(spec, _profile).await
    }

    async fn create_with_security(
        &self,
        spec: &ContainerSpec,
        _profile: &SecurityProfile,
    ) -> Result<ContainerHandle> {
        self.inner.create_with_security(spec, _profile).await
    }
}

fn container(id: &str, project: Option<&str>, service: Option<&str>) -> ContainerInfo {
    let mut labels = HashMap::new();
    if let Some(project) = project {
        labels.insert("perry.compose.project".into(), project.into());
    }
    if let Some(service) = service {
        labels.insert("perry.compose.service".into(), service.into());
    }
    ContainerInfo {
        id: id.into(),
        name: id.into(),
        image: "test".into(),
        status: "running".into(),
        ports: Vec::new(),
        labels,
        created: String::new(),
        ip_address: String::new(),
    }
}

fn engine(containers: Vec<ContainerInfo>) -> (ComposeEngine, Arc<ListingBackend>) {
    let spec = serde_yaml::from_str(
        r#"
services:
  db:
    image: test
    container_name: legacy-db
  web:
    image: test
    container_name: legacy-web
    depends_on: [db]
networks:
  default: {}
"#,
    )
    .unwrap();
    let backend = Arc::new(ListingBackend {
        inner: MockBackend::new(),
        containers,
        fail_list: std::sync::atomic::AtomicBool::new(false),
        arrivals: Mutex::new(Vec::new()),
        arrive_on_stop: Mutex::new(Vec::new()),
    });
    (
        ComposeEngine::new(spec, "project".into(), backend.clone()),
        backend,
    )
}

#[test]
fn down_lists_once_for_multiple_services_and_orphans() {
    let (engine, backend) = engine(vec![
        container("db-1", Some("project"), Some("db")),
        container("foreign-web", Some("other"), Some("web")),
        container("web-1", Some("project"), Some("web")),
        container("unlabelled", None, None),
        container("web-2", Some("project"), Some("web")),
        container("orphan", Some("project"), Some("deleted")),
        container("foreign-orphan", Some("other"), Some("deleted")),
        container("no-service", Some("project"), None),
    ]);
    crate::rt::block_on(engine.down(&[], true, false)).unwrap();
    let calls = backend.inner.recorded_calls();
    let removed: Vec<_> = calls
        .iter()
        .filter_map(|call| match call {
            RecordedCall::Remove(id, true) => Some(id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        removed,
        [
            "web-1",
            "web-2",
            "legacy-web",
            "db-1",
            "legacy-db",
            "orphan"
        ]
    );
    let stopped: Vec<_> = calls
        .iter()
        .filter_map(|call| match call {
            RecordedCall::Stop(id, Some(10)) => Some(id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(stopped, removed);
    let orphan = calls
        .iter()
        .position(|c| matches!(c, RecordedCall::Remove(id, _) if id == "orphan"))
        .unwrap();
    let network = calls
        .iter()
        .position(|c| matches!(c, RecordedCall::RemoveNetwork(name) if name == "project_default"))
        .unwrap();
    assert!(orphan < network);
    assert_eq!(
        calls
            .iter()
            .filter(|c| matches!(c, RecordedCall::List(true)))
            .count(),
        1
    );
}

fn removals(backend: &ListingBackend) -> Vec<String> {
    backend
        .inner
        .recorded_calls()
        .into_iter()
        .filter_map(|call| match call {
            RecordedCall::Remove(id, true) => Some(id),
            _ => None,
        })
        .collect()
}

#[test]
fn down_selected_service_preserves_other_services_and_orphans() {
    let (engine, backend) = engine(vec![
        container("db-1", Some("project"), Some("db")),
        container("web-1", Some("project"), Some("web")),
        container("orphan", Some("project"), Some("deleted")),
    ]);
    crate::rt::block_on(engine.down(&["web".into()], false, false)).unwrap();
    assert_eq!(removals(&backend), ["web-1", "legacy-web"]);
    assert_eq!(
        backend
            .inner
            .recorded_calls()
            .iter()
            .filter(|c| matches!(c, RecordedCall::List(true)))
            .count(),
        1
    );
}

#[test]
fn down_selected_service_with_orphans_preserves_unselected_service() {
    let (engine, backend) = engine(vec![
        container("db-1", Some("project"), Some("db")),
        container("web-1", Some("project"), Some("web")),
        container("orphan", Some("project"), Some("deleted")),
    ]);
    crate::rt::block_on(engine.down(&["web".into()], true, false)).unwrap();
    assert_eq!(removals(&backend), ["web-1", "legacy-web", "orphan"]);
}

#[test]
fn down_without_matching_services_or_orphans_does_not_list() {
    let (engine, backend) = engine(Vec::new());
    crate::rt::block_on(engine.down(&["unknown".into()], false, false)).unwrap();
    assert!(removals(&backend).is_empty());
    assert!(!backend
        .inner
        .recorded_calls()
        .iter()
        .any(|c| matches!(c, RecordedCall::List(_))));
}

#[test]
fn down_empty_snapshot_still_uses_legacy_names() {
    let (engine, backend) = engine(Vec::new());
    crate::rt::block_on(engine.down(&[], false, false)).unwrap();
    assert_eq!(removals(&backend), ["legacy-web", "legacy-db"]);
}

#[test]
fn down_list_failure_aborts_service_teardown() {
    let (engine, backend) = engine(Vec::new());
    backend.fail_list.store(true, Ordering::Relaxed);
    let result = crate::rt::block_on(engine.down(&[], true, false));
    assert!(matches!(result, Err(ComposeError::NotFound(message)) if message == "list failed"));
    assert!(matches!(
        backend.inner.recorded_calls().as_slice(),
        [RecordedCall::List(true)]
    ));
}

#[test]
fn down_orphan_only_list_failure_still_removes_networks() {
    let (engine, backend) = engine(Vec::new());
    backend.fail_list.store(true, Ordering::Relaxed);
    crate::rt::block_on(engine.down(&["unknown".into()], true, false)).unwrap();
    assert!(
        matches!(backend.inner.recorded_calls().as_slice(), [RecordedCall::List(true), RecordedCall::RemoveNetwork(name)] if name == "project_default")
    );
}

#[test]
fn down_orphan_only_sweep_preserves_current_service_containers() {
    let (engine, backend) = engine(vec![
        container("db-1", Some("project"), Some("db")),
        container("orphan", Some("project"), Some("deleted")),
        container("foreign", Some("other"), Some("deleted")),
        container("unlabelled", None, None),
        container("no-service", Some("project"), None),
    ]);
    crate::rt::block_on(engine.down(&["unknown".into()], true, false)).unwrap();
    assert_eq!(removals(&backend), ["orphan"]);
}

#[test]
fn down_leaves_newly_discovered_ids_for_the_next_call() {
    let (engine, backend) = engine(Vec::new());
    *backend.arrive_on_stop.lock().unwrap() = vec![
        container("late-db", Some("project"), Some("db")),
        container("late-orphan", Some("project"), Some("deleted")),
    ];
    crate::rt::block_on(engine.down(&[], true, false)).unwrap();
    assert_eq!(removals(&backend), ["legacy-web", "legacy-db"]);
    backend.inner.calls.lock().unwrap().clear();
    crate::rt::block_on(engine.down(&[], true, false)).unwrap();
    assert_eq!(
        removals(&backend),
        ["legacy-web", "late-db", "legacy-db", "late-orphan"]
    );
    assert_eq!(
        backend
            .inner
            .recorded_calls()
            .iter()
            .filter(|c| matches!(c, RecordedCall::List(true)))
            .count(),
        1
    );
}

#[test]
fn down_rolls_back_before_discovery_and_preserves_resource_policy() {
    for remove_volumes in [false, true] {
        let (mut engine, backend) =
            engine(vec![container("db-snapshot", Some("project"), Some("db"))]);
        engine.spec.volumes = serde_yaml::from_str("data: {}\nshared: {external: true}\n").unwrap();
        engine.spec.networks =
            serde_yaml::from_str("default: {}\nshared: {external: true}\n").unwrap();
        engine
            .session_containers
            .lock()
            .unwrap()
            .extend(["session-old".into(), "session-new".into()]);
        engine
            .session_networks
            .lock()
            .unwrap()
            .push("session-network".into());
        engine
            .session_volumes
            .lock()
            .unwrap()
            .push("session-volume".into());
        *backend.arrive_on_stop.lock().unwrap() =
            vec![container("rollback-arrival", Some("project"), Some("web"))];
        crate::rt::block_on(engine.down(&[], true, remove_volumes)).unwrap();
        let calls = backend.inner.recorded_calls();
        let first_list = calls
            .iter()
            .position(|c| matches!(c, RecordedCall::List(true)))
            .unwrap();
        let prefix: Vec<_> = calls[..first_list]
            .iter()
            .map(|c| format!("{c:?}"))
            .collect();
        let mut expected_prefix = vec![
            "Stop(\"session-new\", Some(5))",
            "Remove(\"session-new\", true)",
            "Stop(\"session-old\", Some(5))",
            "Remove(\"session-old\", true)",
            "RemoveNetwork(\"session-network\")",
        ];
        if remove_volumes {
            expected_prefix.push("RemoveVolume(\"session-volume\")");
        }
        assert_eq!(prefix, expected_prefix);
        assert_eq!(
            removals(&backend),
            [
                "session-new",
                "session-old",
                "rollback-arrival",
                "legacy-web",
                "db-snapshot",
                "legacy-db"
            ]
        );
        let resources: Vec<_> = calls[first_list..]
            .iter()
            .filter_map(|c| match c {
                RecordedCall::RemoveNetwork(n) => Some(format!("network:{n}")),
                RecordedCall::RemoveVolume(n) => Some(format!("volume:{n}")),
                _ => None,
            })
            .collect();
        assert_eq!(
            resources,
            if remove_volumes {
                vec!["network:project_default", "volume:project_data"]
            } else {
                vec!["network:project_default"]
            }
        );
        assert!(engine.session_containers.lock().unwrap().is_empty());
        assert!(engine.session_networks.lock().unwrap().is_empty());
        assert_eq!(
            engine.session_volumes.lock().unwrap().len(),
            if remove_volumes { 0 } else { 1 }
        );
    }
}

#[test]
fn down_rejected_cycle_preserves_backend_spec_and_inventory() {
    let (mut engine, backend) = engine(vec![container("unchanged", Some("project"), Some("db"))]);
    engine.spec.services.get_mut("db").unwrap().depends_on =
        Some(crate::types::DependsOnSpec::List(vec!["web".into()]));
    let spec_before = serde_json::to_string(&engine.spec).unwrap();
    let containers_before = format!("{:?}", backend.containers);
    let error = crate::rt::block_on(engine.down(&[], true, true)).unwrap_err();
    assert!(matches!(error, ComposeError::DependencyCycle { .. }));
    assert!(backend.inner.recorded_calls().is_empty());
    assert_eq!(serde_json::to_string(&engine.spec).unwrap(), spec_before);
    assert_eq!(format!("{:?}", backend.containers), containers_before);
}
