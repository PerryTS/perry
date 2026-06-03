use perry_container_compose::backend::*;
use perry_container_compose::types::ContainerSpec;
use std::collections::HashMap;

// Feature: perry-container | Layer: unit | Req: 1.1 | Property: -
#[test]
fn test_docker_protocol_run_args() {
    let protocol = DockerProtocol;
    let spec = ContainerSpec {
        image: "nginx".into(),
        name: Some("web".into()),
        ports: Some(vec!["80:80".into()]),
        ..Default::default()
    };
    let args = protocol.run_args(&spec);
    assert!(args.contains(&"run".into()));
    assert!(args.contains(&"--name".into()));
    assert!(args.contains(&"web".into()));
    assert!(args.contains(&"80:80".into()));
    assert_eq!(args.last().unwrap(), "nginx");
}

// Feature: perry-container | Layer: unit | Req: 16.1 | Property: -
#[tokio::test]
async fn test_detect_backend_env_override() {
    std::env::set_var("PERRY_CONTAINER_BACKEND", "docker");
    let result = detect_backend().await;
    // This might still fail if docker isn't installed, but it should try ONLY docker
    if let Err(perry_container_compose::error::ComposeError::NoBackendFound { probed }) = result {
        assert_eq!(probed.len(), 1);
        assert_eq!(probed[0].name, "docker");
    }
}

// Coverage Table:
// | Requirement | Test name | Layer |
// |-------------|-----------|-------|
// | 1.1         | test_docker_protocol_run_args | unit |
// | 16.1        | test_detect_backend_env_override | unit |
