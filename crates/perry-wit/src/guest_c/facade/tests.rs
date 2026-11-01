use super::*;

#[cfg(unix)]
#[test]
fn typed_facade_preserves_sync_calls_and_imported_exported_nominal_resources() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("world.wit");
    std::fs::write(
        &path,
        r#"package test:producer-facade;
        interface api {
            resource item { constructor(value: u32); read: func() -> u32; }
            record payload { value: own<item>, text: string }
            make: func() -> stream<payload>;
            later: async func() -> u32;
        }
        world plugin {
            import api;
            export api;
            record input { wide: u64, text: string }
            export wrap: func(value: input) -> future<input>;
        }"#,
    )
    .unwrap();
    assert!(canonical_guest(&path, None).is_err());
    let files = experimental_async_guest(&path, None).unwrap();
    let plan: Plan = serde_json::from_slice(&files["perry_producer_facade.json"]).unwrap();
    assert_eq!(plan.forwards.len(), 4);
    assert!(plan
        .forwards
        .iter()
        .any(|f| f.function == "[constructor]item"));
    assert!(plan
        .forwards
        .iter()
        .any(|f| f.function == "[method]item.read"));
    assert!(!plan.forwards.iter().any(|f| f.function == "later"));
    for (name, bytes) in &files {
        if name.ends_with("_component_type.o") {
            wit_component::metadata::decode(bytes)
                .expect("facade and producer metadata must decode");
        }
        std::fs::write(dir.path().join(name), bytes).unwrap();
    }
    let output = std::process::Command::new("clang")
        .args([
            "-std=c11",
            "-Wall",
            "-Werror",
            "-Wno-unused-function",
            "-fsyntax-only",
        ])
        .arg(dir.path().join("perry_producer_forward.c"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
