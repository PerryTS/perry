use perry_container_compose::compose::ComposeEngine;
use perry_container_compose::types::{ComposeService, ComposeSpec};
use std::sync::Arc;

mod common;
use common::MockBackend;

#[tokio::test]
async fn test_compose_up_success() {
    let mut spec = ComposeSpec::default();
    spec.services.insert(
        "web".into(),
        ComposeService {
            image: Some("nginx".into()),
            ..Default::default()
        },
    );
    spec.services.insert(
        "db".into(),
        ComposeService {
            image: Some("postgres".into()),
            ..Default::default()
        },
    );

    let backend = Arc::new(MockBackend::default());
    let engine = Arc::new(ComposeEngine::new(
        spec,
        "test-project".into(),
        backend.clone(),
    ));

    let handle = Arc::clone(&engine)
        .up(&[], true, false, false)
        .await
        .expect("up failed");

    assert_eq!(handle.project_name, "test-project");
    assert_eq!(handle.services.len(), 2);

    let state = backend.state.lock().unwrap();
    assert_eq!(state.containers.len(), 2);
}

#[tokio::test]
async fn test_compose_up_rollback_on_failure() {
    let mut spec = ComposeSpec::default();
    spec.services.insert(
        "db".into(),
        ComposeService {
            image: Some("postgres".into()),
            ..Default::default()
        },
    );
    spec.services.insert(
        "web".into(),
        ComposeService {
            image: Some("nginx".into()),
            ..Default::default()
        },
    );

    let backend = Arc::new(MockBackend::default());
    {
        let mut state = backend.state.lock().unwrap();
        // Since we don't know the exact generated name, we fail if the image name 'nginx' is in the spec
        state.fail_on_run = Some("nginx".into());
    }

    let engine = Arc::new(ComposeEngine::new(
        spec,
        "fail-project".into(),
        backend.clone(),
    ));
    let result = Arc::clone(&engine).up(&[], true, false, false).await;

    assert!(
        result.is_err(),
        "Result should be an error because 'web' service (nginx) was set to fail"
    );

    let state = backend.state.lock().unwrap();
    // Should have started db, tried web, then stopped/removed db
    assert!(
        state.containers.is_empty(),
        "Containers should be empty after rollback, but found: {:?}",
        state.containers
    );

    let actions: Vec<_> = state
        .actions
        .iter()
        .map(|s| s.split(':').next().unwrap())
        .collect();
    assert!(actions.contains(&"run")); // db
    assert!(actions.contains(&"stop")); // db rollback
    assert!(actions.contains(&"remove")); // db rollback
}

#[tokio::test]
async fn test_compose_down_cleans_resources() {
    let mut spec = ComposeSpec::default();
    spec.services.insert(
        "web".into(),
        ComposeService {
            image: Some("nginx".into()),
            ..Default::default()
        },
    );

    let backend = Arc::new(MockBackend::default());
    let engine = Arc::new(ComposeEngine::new(
        spec,
        "down-project".into(),
        backend.clone(),
    ));

    let _handle = Arc::clone(&engine)
        .up(&[], true, false, false)
        .await
        .unwrap();

    // down() should use resolve_startup_order and clean up
    engine.down(&[], false, true).await.expect("down failed");

    let state = backend.state.lock().unwrap();
    // In our MockBackend, remove just deletes the container from the map.
    assert!(
        state.containers.is_empty(),
        "Containers should be empty, but found: {:?}",
        state.containers
    );
}

#[tokio::test]
async fn test_compose_project_name_scopes_volumes_networks_and_labels() {
    // The project name (ComposeSpec.name via the FFI; the second
    // ComposeEngine::new arg here) must namespace non-external volumes
    // and networks as `<project>_<declared-name>` and stamp the
    // `perry.compose.project` label on every container. Typed TS
    // callers couldn't set it before ComposeSpec.name landed in the
    // d.ts — every stack silently collided under "perry-stack".
    use perry_container_compose::types::ServiceNetworks;

    let mut spec = ComposeSpec::default();
    spec.services.insert(
        "web".into(),
        ComposeService {
            image: Some("nginx".into()),
            volumes: Some(vec![serde_yaml::Value::String("data:/var/www".into())]),
            networks: Some(ServiceNetworks::List(vec!["appnet".into()])),
            ..Default::default()
        },
    );
    spec.volumes = Some({
        let mut m = indexmap::IndexMap::new();
        m.insert("data".to_string(), None);
        m
    });
    spec.networks = Some({
        let mut m = indexmap::IndexMap::new();
        m.insert("appnet".to_string(), None);
        m
    });

    let backend = Arc::new(MockBackend::default());
    let engine = Arc::new(ComposeEngine::new(spec, "myproj".into(), backend.clone()));
    Arc::clone(&engine)
        .up(&[], true, false, false)
        .await
        .expect("up failed");

    let state = backend.state.lock().unwrap();
    assert!(
        state.volumes.contains(&"myproj_data".to_string()),
        "volume must be project-scoped as myproj_data; got {:?}",
        state.volumes
    );
    assert!(
        state.networks.contains(&"myproj_appnet".to_string()),
        "network must be project-scoped as myproj_appnet; got {:?}",
        state.networks
    );
    let web = state
        .containers
        .values()
        .next()
        .expect("one container expected");
    assert_eq!(
        web.labels.get("perry.compose.project"),
        Some(&"myproj".to_string()),
        "container must carry the project label"
    );
}

/// Seed the mock backend with a container that looks like a leftover
/// from an earlier deploy of `project`: it carries Perry's compose
/// labels but its service key is not in the current spec.
fn seed_orphan(backend: &MockBackend, id: &str, project: &str, service: &str) {
    use perry_container_compose::types::ContainerInfo;
    let mut labels = std::collections::HashMap::new();
    labels.insert("perry.compose.project".to_string(), project.to_string());
    labels.insert("perry.compose.service".to_string(), service.to_string());
    backend.state.lock().unwrap().containers.insert(
        id.to_string(),
        ContainerInfo {
            id: id.to_string(),
            name: id.to_string(),
            image: "busybox".to_string(),
            status: "running".to_string(),
            ports: vec![],
            labels,
            created: "2025-01-01T00:00:00Z".to_string(),
            ip_address: String::new(),
        },
    );
}

#[tokio::test]
async fn test_compose_down_remove_orphans_removes_stale_service_containers() {
    // A container from a previous deploy whose service key
    // ("old-worker") was deleted from the spec must be stopped +
    // removed when down() runs with remove_orphans = true. Pre-fix the
    // flag was parsed at the FFI and discarded (`_remove_orphans`).
    let mut spec = ComposeSpec::default();
    spec.services.insert(
        "web".into(),
        ComposeService {
            image: Some("nginx".into()),
            ..Default::default()
        },
    );

    let backend = Arc::new(MockBackend::default());
    seed_orphan(&backend, "orphan-ctr", "orphan-proj", "old-worker");

    let engine = Arc::new(ComposeEngine::new(
        spec,
        "orphan-proj".into(),
        backend.clone(),
    ));
    let _ = Arc::clone(&engine)
        .up(&[], true, false, false)
        .await
        .expect("up failed");

    engine.down(&[], true, false).await.expect("down failed");

    let state = backend.state.lock().unwrap();
    assert!(
        !state.containers.contains_key("orphan-ctr"),
        "orphan must be removed by down(remove_orphans: true); got {:?}",
        state.containers.keys().collect::<Vec<_>>()
    );
    assert!(
        state.actions.iter().any(|a| a == "remove:orphan-ctr"),
        "expected an explicit remove of the orphan; actions: {:?}",
        state.actions
    );
}

#[tokio::test]
async fn test_compose_down_without_remove_orphans_keeps_orphans() {
    // Default behavior is unchanged: remove_orphans = false leaves the
    // stale container alone (only current-spec services are removed).
    let mut spec = ComposeSpec::default();
    spec.services.insert(
        "web".into(),
        ComposeService {
            image: Some("nginx".into()),
            ..Default::default()
        },
    );

    let backend = Arc::new(MockBackend::default());
    seed_orphan(&backend, "orphan-ctr", "orphan-proj", "old-worker");

    let engine = Arc::new(ComposeEngine::new(
        spec,
        "orphan-proj".into(),
        backend.clone(),
    ));
    let _ = Arc::clone(&engine)
        .up(&[], true, false, false)
        .await
        .expect("up failed");

    engine.down(&[], false, false).await.expect("down failed");

    let state = backend.state.lock().unwrap();
    assert!(
        state.containers.contains_key("orphan-ctr"),
        "down without remove_orphans must NOT touch the orphan"
    );
}

#[tokio::test]
async fn test_compose_down_remove_orphans_scoped_to_project_and_labels() {
    // The orphan sweep must be strictly label-scoped: containers from
    // OTHER projects and containers without Perry's compose labels are
    // never candidates, even with remove_orphans = true.
    use perry_container_compose::types::ContainerInfo;

    let mut spec = ComposeSpec::default();
    spec.services.insert(
        "web".into(),
        ComposeService {
            image: Some("nginx".into()),
            ..Default::default()
        },
    );

    let backend = Arc::new(MockBackend::default());
    // Same-key orphan in a DIFFERENT project.
    seed_orphan(&backend, "other-proj-ctr", "some-other-proj", "old-worker");
    // Unlabelled container (not created by Perry at all).
    backend.state.lock().unwrap().containers.insert(
        "unlabelled-ctr".to_string(),
        ContainerInfo {
            id: "unlabelled-ctr".to_string(),
            name: "unlabelled-ctr".to_string(),
            image: "busybox".to_string(),
            status: "running".to_string(),
            ports: vec![],
            labels: std::collections::HashMap::new(),
            created: "2025-01-01T00:00:00Z".to_string(),
            ip_address: String::new(),
        },
    );

    let engine = Arc::new(ComposeEngine::new(
        spec,
        "orphan-proj".into(),
        backend.clone(),
    ));
    let _ = Arc::clone(&engine)
        .up(&[], true, false, false)
        .await
        .expect("up failed");

    engine.down(&[], true, false).await.expect("down failed");

    let state = backend.state.lock().unwrap();
    assert!(
        state.containers.contains_key("other-proj-ctr"),
        "other project's container must survive the orphan sweep"
    );
    assert!(
        state.containers.contains_key("unlabelled-ctr"),
        "unlabelled container must survive the orphan sweep"
    );
}
