use semver::Version;
use wac_types::{validate_target, Package, Types};

#[test]
fn test_target_validation() {
    let mut types = Types::default();

    let wasi_http_020_version: Version = "0.2.0".parse().unwrap();
    let wasi_http_020_component = Package::from_file(
        "wasi:http",
        Some(&wasi_http_020_version),
        "tests/dummy_wasi_http@0.2.0.wasm",
        &mut types,
    )
    .unwrap();

    let wasi_http_023_version: Version = "0.2.3".parse().unwrap();
    let wasi_http_023_component = Package::from_file(
        "wasi:http",
        Some(&wasi_http_023_version),
        "tests/dummy_wasi_http@0.2.3.wasm",
        &mut types,
    )
    .unwrap();

    assert!(
        validate_target(
            &types,
            wasi_http_023_component.ty(),
            wasi_http_020_component.ty(),
        )
        .is_ok(),
        "Target validation should pass for backward-compatible versions",
    );

    assert!(
        validate_target(
            &types,
            wasi_http_020_component.ty(),
            wasi_http_023_component.ty(),
        )
        .is_err(),
        "Target validation should fail for backward-incompatible versions",
    );
}
