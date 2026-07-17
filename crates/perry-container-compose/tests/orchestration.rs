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
