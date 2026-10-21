use super::*;

const TRANSFORM: &str = include_str!("../../../test-wasi/components/transform/world.wit");

#[test]
fn preserves_versioned_interfaces_and_structured_types() {
    let world = parse(TRANSFORM, Some("plugin")).unwrap();
    assert_eq!(world.name, "perry:transform/plugin@1.0.0");
    assert_eq!(world.imports[0].name, "perry:transform/logging@1.0.0");
    assert_eq!(world.exports[0].name, "perry:transform/transform@1.0.0");
    assert!(world
        .types
        .values()
        .any(|d| matches!(d.ty, Type::Record(_))));
    assert!(world
        .types
        .values()
        .any(|d| matches!(d.ty, Type::Result { .. })));
    let json = serde_json::to_string(&world).unwrap();
    assert_eq!(serde_json::from_str::<World>(&json).unwrap(), world);
    assert_eq!(
        typescript(&world),
        typescript(&parse(TRANSFORM, None).unwrap())
    );
}

#[test]
fn adapters_keep_both_call_directions_and_do_not_erase_types() {
    let world = parse(TRANSFORM, None).unwrap();
    let host = typescript(&world);
    let guest = typescript_guest(&world);
    assert!(!host.contains("registerComponentExports"));
    assert!(!host.contains("callComponentImport"));
    assert!(!guest.contains("loadComponent"));
    assert!(!guest.contains("disposeComponent"));
    let ts = format!("{host}\n{guest}");
    assert!(ts.contains("Uint8Array"));
    assert!(ts.contains("{ tag: \"err\"; value:"));
    assert!(ts.contains(
        "callComponent(handle, \"perry:transform/transform@1.0.0\", \"transform\", [arg0])"
    ));
    assert!(ts.contains("callComponentImport(\"perry:transform/logging@1.0.0\", \"log\", [arg0])"));
    assert!(ts.contains("registerComponentExports(contract, exports)"));
    assert!(!ts.contains("any"));
}

#[test]
fn options_preserve_some_none_and_integer_precision() {
    let world = parse("package test:values; world values { export value: func(input: option<option<u64>>) -> tuple<s64, f64>; }", None).unwrap();
    let ts = typescript(&world);
    assert!(ts.contains("bigint"));
    assert!(ts.contains("[bigint, number]"));
    assert_eq!(ts.matches("{ tag: \"none\" }").count(), 2);
    assert_eq!(ts.matches("{ tag: \"some\"; value:").count(), 2);
}

#[test]
fn byte_alias_lists_keep_the_runtime_uint8array_representation() {
    let world = parse(
        "package test:bytes; interface data { type octet = u8; type byte-alias = octet; \
         echo: func(input: list<byte-alias>) -> list<byte-alias>; } \
         world plugin { export data; }",
        None,
    )
    .unwrap();
    for ts in [typescript(&world), typescript_guest(&world)] {
        assert!(ts.contains("= Uint8Array;"), "{ts}");
        assert!(!ts.contains("Array<"), "{ts}");
    }
}

#[test]
fn resources_keep_identity_ownership_and_method_signatures() {
    let world = parse(
        r#"package test:resources@2.0.0;
        interface handles {
          resource item { constructor(); read: func() -> string; }
          pass: func(value: own<item>) -> own<item>;
        }
        world plugin { export handles; }"#,
        None,
    )
    .unwrap();
    let interface = &world.exports[0];
    assert_eq!(interface.resources.len(), 1);
    assert!(interface
        .functions
        .iter()
        .any(|f| matches!(f.kind, FunctionKind::Constructor(_))));
    let method = interface
        .functions
        .iter()
        .find(|f| matches!(f.kind, FunctionKind::Method(_)))
        .unwrap();
    assert_eq!(method.params[0].name, "self");
    let ts = typescript(&world);
    assert!(ts.contains("BorrowedResource<\"test:resources/plugin@2.0.0/"));
    assert!(ts.contains("OwnedResource<\"test:resources/plugin@2.0.0/"));
    assert!(!ts.contains("= never"));
}

#[test]
fn rejects_async_types_instead_of_generating_any() {
    for signature in [
        "func() -> future<string>",
        "func() -> stream<u8>",
        "async func() -> string",
    ] {
        let source =
            format!("package test:async-types; world plugin {{ export call: {signature}; }}");
        let error = parse(&source, None).unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.contains("P3") || message.contains("P2 binding profile"),
            "{message}"
        );
    }
}

#[test]
fn directory_resolution_and_world_selection_are_explicit() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("world.wit"),
        "package test:multi; world first {} world second {}",
    )
    .unwrap();
    assert!(parse_file(dir.path(), None).is_err());
    assert_eq!(
        parse_file(dir.path(), Some("second")).unwrap().name,
        "test:multi/second"
    );
}

#[cfg(unix)]
#[test]
fn canonical_guest_adapters_compile_against_the_maintained_c_headers() {
    let worlds = [
        TRANSFORM,
        r#"
        package test:all-values@1.2.3;
        interface values {
            type octet = u8;
            type bytes = list<octet>;
            flags permissions { read, write, execute }
            enum color { red, green, blue }
            variant choice { absent, text(string), colors(list<color>) }
            record data {
                a: s8, b: u8, c: s16, d: u16, e: s32, f: u32,
                g: s64, h: u64, i: f32, j: f64, k: char, l: bool,
                text: string, bytes: bytes, permissions: permissions,
                choice: choice, nested: option<option<string>>,
                pair: tuple<string, list<u16>>,
            }
            echo: func(value: data) -> result<data, choice>;
            notify: func(value: string);
            scalar: func() -> u64;
        }
        world plugin { import values; export values; }
    "#,
        r#"
        package test:root-functions;
        world plugin {
            type payload = tuple<string, list<u32>>;
            import log: func(value: payload);
            export echo: func(value: payload) -> payload;
            export empty: func();
        }
    "#,
    ];
    for source in worlds {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("world.wit");
        std::fs::write(&input, source).unwrap();
        let files = canonical_guest(&input, None).unwrap();
        assert!(files.keys().any(|name| name.ends_with("_component_type.o")));
        for (name, contents) in &files {
            std::fs::write(dir.path().join(name), contents).unwrap();
        }
        let output = std::process::Command::new("clang")
            .args([
                "-std=c11",
                "-Wall",
                "-Werror",
                "-Wno-unused-function",
                "-fsyntax-only",
            ])
            .arg(dir.path().join("perry_adapter.c"))
            .output()
            .expect("clang is required for the canonical C adapter test");
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&files["perry_adapter.c"])
        );
    }
}

#[test]
fn canonical_guest_resource_adapters_compile_for_both_directions() {
    for world in ["import api;", "export api;", "import api; export api;"] {
        let source = format!(
            r#"package test:resources;
            interface api {{
                resource item {{ constructor(value: string); read: func() -> string; }}
                record bundle {{ label: string, items: list<own<item>> }}
                take: func(value: bundle) -> own<item>;
                inspect: func(value: borrow<item>) -> string;
            }}
            world plugin {{ {world} }}"#
        );
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("world.wit");
        std::fs::write(&input, &source).unwrap();
        let files = canonical_guest(&input, None).unwrap();
        for (name, contents) in &files {
            std::fs::write(dir.path().join(name), contents).unwrap();
        }
        let output = std::process::Command::new("clang")
            .args([
                "-std=c11",
                "-Wall",
                "-Werror",
                "-Wno-unused-function",
                "-fsyntax-only",
            ])
            .arg(dir.path().join("perry_adapter.c"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&files["perry_adapter.c"])
        );
        let c = String::from_utf8(files["perry_adapter.c"].clone()).unwrap();
        assert!(c.contains("perry_component_frame_resource_commit(f)"));
        let contract = parse(&source, None).unwrap();
        if world.contains("import api;") && world.contains("export api;") {
            assert_ne!(
                contract.imports[0].resources["item"], contract.exports[0].resources["item"],
                "imported and exported resources are distinct nominal types"
            );
        }
        for interface in &contract.exports {
            for key in interface
                .resources
                .values()
                .filter(|key| key.starts_with('e'))
            {
                let id = key[1..].parse::<u32>().unwrap() * 2 + 1;
                assert!(
                    c.contains(&format!("case {id}:\n")),
                    "guest factory {key} must select its generated canonical resource operation"
                );
            }
        }
        let guest = typescript_guest(&contract);
        assert_eq!(
            guest.contains("create: (payload:"),
            world.contains("export api;")
        );
        // The maintained backend frees transferred buffers separately from
        // explicit resource-drop helpers. A list cleanup must not drop owners
        // which the runtime has adopted.
        let core = String::from_utf8(files["plugin.c"].clone()).unwrap();
        let free = core.split("void test_resources_api_bundle_free(").nth(1);
        if let Some(free) = free {
            assert!(!free.split("\n}\n").next().unwrap().contains("drop_own"));
        }
    }
}

#[test]
fn flags_respect_the_component_binary_format_limit() {
    for count in [1, 32, 33, 64] {
        let flags = (0..count)
            .map(|i| format!("bit{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let source = format!("package test:flag-profile; interface api {{ flags bits {{ {flags} }} echo: func(value: bits) -> bits; }} world plugin {{ import api; export api; }}");
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("world.wit");
        std::fs::write(&input, &source).unwrap();
        if count <= 32 {
            parse(&source, None).unwrap();
            assert!(canonical_guest(&input, None).is_ok());
        } else {
            assert!(parse(&source, None)
                .unwrap_err()
                .to_string()
                .contains("32 flags"));
            assert!(canonical_guest(&input, None)
                .unwrap_err()
                .to_string()
                .contains("32 flags"));
        }
    }
}

#[test]
fn provider_imports_can_be_omitted_without_erasing_application_requirements() {
    let mut world = parse(TRANSFORM, None).unwrap();
    let mut wasi = world.imports[0].clone();
    wasi.name = "wasi:cli/environment@0.2.0".into();
    world.imports.push(wasi);
    let host = typescript(&world);
    let application = host
        .split("export interface ApplicationImports")
        .nth(1)
        .unwrap()
        .split("export interface Component")
        .next()
        .unwrap();
    assert!(application.contains("perry:transform/logging@1.0.0"));
    assert!(!application.contains("wasi:"));
    assert!(typescript_guest(&world).contains("wasi:cli/environment@0.2.0"));
}

#[test]
fn resource_aliases_preserve_nominal_identity_and_compile() {
    let source = r#"package test:resource-alias;
        interface origin { resource item { constructor(); read: func() -> u32; } }
        interface api {
            use origin.{item as renamed};
            forward: func(value: own<renamed>) -> own<renamed>;
            inspect: func(value: borrow<renamed>) -> u32;
        }
        world plugin { import origin; export api; }
    "#;
    let world = parse(source, None).unwrap();
    assert_eq!(
        world.imports[0].resources["item"],
        world.exports[0].resources["renamed"]
    );
    let guest = typescript_guest(&world);
    assert!(
        !guest.contains("create: (payload:"),
        "an alias must not introduce a new resource factory"
    );
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("world.wit");
    std::fs::write(&input, source).unwrap();
    let files = canonical_guest(&input, None).unwrap();
    for (name, content) in &files {
        std::fs::write(dir.path().join(name), content).unwrap();
    }
    let output = std::process::Command::new("clang")
        .args([
            "-std=c11",
            "-Wall",
            "-Werror",
            "-Wno-unused-function",
            "-fsyntax-only",
        ])
        .arg(dir.path().join("perry_adapter.c"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&files["perry_adapter.c"])
    );
}
