use super::*;

#[test]
fn private_source_interface_preserves_public_package_names() {
    let template = r#"package test:facade-package;
        interface api { inspect: func() -> u32; later: async func() -> u32; }
        interface reserved { original: func() -> u64; }
        world plugin { import api; import reserved; export api; }"#;
    let mut resolve = Resolve::default();
    let package = resolve.push_str("world.wit", template).unwrap();
    let world = resolve.select_world(&[package], None).unwrap();
    resolve.generate_nominal_type_ids(world);
    let exported = resolve.worlds[world]
        .exports
        .values()
        .find_map(|item| match item {
            WorldItem::Interface { id, .. } => Some(*id),
            _ => None,
        })
        .unwrap();
    assert!(resolve.interfaces[exported].clone_of.is_some());
    let reserved = format!("perry-producer-source-{}", exported.index());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("world.wit");
    std::fs::write(&path, template.replace("reserved", &reserved)).unwrap();
    let files = experimental_async_guest(&path, None).unwrap();
    let plan: Plan = serde_json::from_slice(&files["perry_producer_facade.json"]).unwrap();
    let renamed = plan
        .imports
        .iter()
        .find(|binding| binding.original == "test:facade-package/api")
        .unwrap();
    assert_eq!(renamed.local, format!("test:facade-package/{reserved}-1"));
    let (_, metadata) =
        wit_component::metadata::decode(&files["perry_producer_facade_component_type.o"]).unwrap();
    let original = metadata
        .resolve
        .interfaces
        .iter()
        .find(|(_, interface)| interface.name.as_deref() == Some(reserved.as_str()))
        .unwrap()
        .1;
    assert!(original.functions.contains_key("original"));
    assert!(!original.functions.contains_key("inspect"));
}

#[test]
fn private_forward_interface_preserves_labeled_imports() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("world.wit");
    std::fs::write(
        &path,
        r#"package test:facade-labels;
        world plugin {
            import perry-producer-forward: interface { first: func() -> u32; }
            import perry-producer-forward-1: interface { second: func() -> u64; }
            export inspect: func() -> u32;
            export later: async func() -> u32;
        }"#,
    )
    .unwrap();
    let files = experimental_async_guest(&path, None).unwrap();
    let plan: Plan = serde_json::from_slice(&files["perry_producer_facade.json"]).unwrap();
    assert_eq!(plan.forward_interface, "perry-producer-forward-2");
    let (_, metadata) =
        wit_component::metadata::decode(&files["perry_producer_facade_component_type.o"]).unwrap();
    let world = &metadata.resolve.worlds[metadata.world];
    for (label, function) in [
        ("perry-producer-forward", "first"),
        ("perry-producer-forward-1", "second"),
    ] {
        let WorldItem::Interface { id, .. } = world.imports[&WorldKey::Name(label.into())] else {
            panic!("a public labeled import must remain an interface");
        };
        assert!(metadata.resolve.interfaces[id]
            .functions
            .contains_key(function));
    }
    assert!(world
        .exports
        .contains_key(&WorldKey::Name(plan.forward_interface)));
}

#[test]
fn private_forward_names_preserve_public_world_type_identities() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("world.wit");
    std::fs::write(
        &path,
        r#"package test:facade-names;
        world plugin {
            record perry-forward-0 { value: u32 }
            record perry-source-0 { wide: u64, text: string }
            export inspect: func(value: perry-forward-0) -> u32;
            export later: async func(value: perry-source-0) -> perry-source-0;
        }"#,
    )
    .unwrap();
    let files = experimental_async_guest(&path, None).unwrap();
    let plan: Plan = serde_json::from_slice(&files["perry_producer_facade.json"]).unwrap();
    assert_eq!(plan.forwards.len(), 1);
    assert_eq!(plan.forwards[0].function, "inspect");
    let (_, metadata) =
        wit_component::metadata::decode(&files["perry_producer_facade_component_type.o"]).unwrap();
    let world = &metadata.resolve.worlds[metadata.world];
    let WorldItem::Type { id, .. } = world.imports[&WorldKey::Name("perry-forward-0".into())]
    else {
        panic!("the public record must remain a type import");
    };
    let WorldItem::Function(forwarded) =
        &world.exports[&WorldKey::Name(plan.forwards[0].export.clone())]
    else {
        panic!("the private forward must remain callable");
    };
    assert_eq!(forwarded.params[0].ty, wit_parser::Type::Id(id));
    assert!(matches!(
        metadata.resolve.types[id].kind,
        TypeDefKind::Record(_)
    ));
}

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
