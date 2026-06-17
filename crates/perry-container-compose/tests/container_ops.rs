use perry_container_compose::types::ContainerSpec;
use perry_container_compose::ContainerBackend;
use std::sync::Arc;

mod common;
use common::MockBackend;

#[tokio::test]
async fn test_container_run_success() {
    let mock = MockBackend::default();
    let state_ref = Arc::clone(&mock.state);
    let backend: Arc<dyn ContainerBackend> = Arc::new(mock);
    let spec = ContainerSpec {
        image: "alpine".into(),
        name: Some("test-container".into()),
        ..Default::default()
    };

    let handle = backend.run(&spec).await.expect("run failed");
    assert_eq!(handle.id, "test-container");

    let state = state_ref.lock().unwrap();
    assert!(state.containers.contains_key("test-container"));
    assert_eq!(state.actions, vec!["run:test-container"]);
}

#[tokio::test]
async fn test_container_lifecycle() {
    let mock = MockBackend::default();
    let state_ref = Arc::clone(&mock.state);
    let backend: Arc<dyn ContainerBackend> = Arc::new(mock);
    let spec = ContainerSpec {
        image: "nginx".into(),
        name: Some("web".into()),
        ..Default::default()
    };

    backend.run(&spec).await.unwrap();
    backend.stop("web", Some(10)).await.unwrap();
    backend.remove("web", true).await.unwrap();

    let state = state_ref.lock().unwrap();
    assert!(state.containers.is_empty());
    assert_eq!(state.actions, vec!["run:web", "stop:web", "remove:web"]);
}

#[tokio::test]
async fn test_container_exec() {
    let backend: Arc<dyn ContainerBackend> = Arc::new(MockBackend::default());
    let logs = backend
        .exec("web", &["ls".into()], None, None)
        .await
        .unwrap();
    assert_eq!(logs.stdout, "exec");
}

#[tokio::test]
async fn test_network_volume_lifecycle() {
    let mock = MockBackend::default();
    let state_ref = Arc::clone(&mock.state);
    let backend: Arc<dyn ContainerBackend> = Arc::new(mock);
    use perry_container_compose::types::{ComposeNetwork, ComposeVolume};

    backend
        .create_network("test-net", &ComposeNetwork::default())
        .await
        .unwrap();
    backend
        .create_volume("test-vol", &ComposeVolume::default())
        .await
        .unwrap();

    {
        let state = state_ref.lock().unwrap();
        assert_eq!(state.networks, vec!["test-net"]);
        assert_eq!(state.volumes, vec!["test-vol"]);
    }

    backend.remove_network("test-net").await.unwrap();
    backend.remove_volume("test-vol").await.unwrap();

    {
        let state = state_ref.lock().unwrap();
        assert!(state.networks.is_empty());
        assert!(state.volumes.is_empty());
    }
}
