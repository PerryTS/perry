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
