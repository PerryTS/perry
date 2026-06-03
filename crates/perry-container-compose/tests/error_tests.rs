use perry_container_compose::error::{compose_error_to_js, ComposeError};

// Feature: perry-container | Layer: unit | Req: 2.6 | Property: 11
#[test]
fn test_compose_error_to_js_not_found() {
    let err = ComposeError::NotFound("resource".into());
    let js = compose_error_to_js(&err);
    assert!(js.contains("\"code\":404"));
    assert!(js.contains("resource"));
}

// Feature: perry-container | Layer: unit | Req: 9.8 | Property: 11
#[test]
fn test_compose_error_to_js_file_not_found() {
    let err = ComposeError::FileNotFound {
        path: "config.yaml".into(),
    };
    let js = compose_error_to_js(&err);
    assert!(js.contains("\"code\":404"));
    assert!(js.contains("config.yaml"));
}

// Feature: perry-container | Layer: unit | Req: 2.6 | Property: 11
#[test]
fn test_compose_error_to_js_backend_error() {
    let err = ComposeError::BackendError {
        code: 127,
        message: "command not found".into(),
    };
    let js = compose_error_to_js(&err);
    assert!(js.contains("\"code\":127"));
    assert!(js.contains("command not found"));
}

// Feature: perry-container | Layer: unit | Req: 6.5 | Property: 11
#[test]
fn test_compose_error_to_js_dependency_cycle() {
    let err = ComposeError::DependencyCycle {
        services: vec!["a".into(), "b".into()],
    };
    let js = compose_error_to_js(&err);
    assert!(js.contains("\"code\":422"));
    assert!(js.contains("a"));
    assert!(js.contains("b"));
}

// Feature: perry-container | Layer: unit | Req: 6.10 | Property: 11
#[test]
fn test_compose_error_to_js_startup_failed() {
    let err = ComposeError::ServiceStartupFailed {
        service: "web".into(),
        message: "exit 1".into(),
    };
    let js = compose_error_to_js(&err);
    assert!(js.contains("\"code\":500"));
}

// Feature: perry-container | Layer: unit | Req: 16.11 | Property: 11
#[test]
fn test_compose_error_to_js_no_backend() {
    let err = ComposeError::NoBackendFound { probed: vec![] };
    let js = compose_error_to_js(&err);
    assert!(js.contains("\"code\":503"));
}

// Coverage Table:
// | Requirement | Test name | Layer |
// |-------------|-----------|-------|
// | 2.6         | test_compose_error_to_js_not_found | unit |
// | 2.6         | test_compose_error_to_js_backend_error | unit |
// | 6.5         | test_compose_error_to_js_dependency_cycle | unit |
// | 6.10        | test_compose_error_to_js_startup_failed | unit |
// | 9.8         | test_compose_error_to_js_file_not_found | unit |
// | 16.11       | test_compose_error_to_js_no_backend | unit |
