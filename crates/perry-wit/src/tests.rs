use super::*;

const TRANSFORM: &str = include_str!("../../../test-wasi/components/transform/world.wit");

#[test]
fn imported_resource_factories_keep_aliases_without_inventing_world_imports() {
    let world = parse(
        r#"package test:host;
        interface items { resource item; }
        interface aliases { use items.{item as alias}; }
        world plugin {
            import items;
            import aliases;
            use items.{item};
            export echo: func(value: own<item>) -> own<item>;
        }"#,
        None,
    )
    .unwrap();
    // A world-level use is a type alias, not an additional canonical import.
    assert!(world.imports.iter().all(|i| !i.name.is_empty()));
    let items = world
        .imports
        .iter()
        .find(|i| i.name == "test:host/items")
        .unwrap();
    let aliases = world
        .imports
        .iter()
        .find(|i| i.name == "test:host/aliases")
        .unwrap();
    assert_eq!(items.resources["item"], aliases.resources["alias"]);
    let ts = typescript(&world);
    assert!(ts.contains("export interface HostResources"));
    assert!(ts.contains("imports(hostResources(handle))"));
    for interface in &world.imports {
        for key in interface.resources.values() {
            assert!(ts.contains(&format!("createComponentHostResource(handle, \"{key}\"")));
        }
    }
}

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
fn async_values_keep_their_payload_and_are_not_promise_aliases() {
    for signature in [
        "async func() -> future<string>",
        "func() -> stream<u8>",
        "func() -> future",
        "func() -> stream",
    ] {
        let source =
            format!("package test:async-types; world plugin {{ export call: {signature}; }}");
        let world = parse(&source, None).unwrap();
        let ts = typescript(&world);
        assert!(!ts.contains("any"));
        if signature.starts_with("async") {
            assert!(ts.contains("Promise<"));
        }
        if signature.contains("future") {
            assert!(ts.contains("= ComponentFuture<"));
        }
        if signature.contains("stream<u8>") {
            assert!(ts.contains("= ComponentStream<number, Uint8Array>"));
        }
        assert_eq!(
            serde_json::from_str::<World>(&serde_json::to_string(&world).unwrap()).unwrap(),
            world
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

#[test]
fn incoming_http_host_binding_uses_the_selected_interface_version() {
    let world = parse(
        "package wasi:http@0.2.12; interface incoming-handler { handle: func(); } world proxy { export incoming-handler; }",
        None,
    ).unwrap();
    let host = typescript(&world);
    assert!(host.contains("handleHttp(server: ComponentHttpServer): Promise<void>"));
    assert!(host.contains(
        "handleComponentHttpRequest(server, handle, \"wasi:http/incoming-handler@0.2.12\")"
    ));
    assert!(!typescript_guest(&world).contains("handleComponentHttpRequest"));
    assert!(!typescript(&parse(TRANSFORM, None).unwrap()).contains("handleHttp"));
}

#[test]
fn async_calling_conventions_survive_manifest_round_trips_and_legacy_defaults() {
    let world = parse("package test:async-calls; interface jobs { resource job { run: async func() -> u32; make: static async func() -> own<job>; } execute: async func(value: u32) -> u32; } world plugin { export jobs; }", None).unwrap();
    assert!(world.exports[0].functions.iter().all(|f| f.async_));
    let json = serde_json::to_string(&world).unwrap();
    assert!(json.contains("\"async\":true"));
    assert_eq!(serde_json::from_str::<World>(&json).unwrap(), world);
    let legacy = parse(TRANSFORM, None).unwrap();
    let json = serde_json::to_string(&legacy).unwrap();
    assert!(!json.contains("\"async\":"));
    assert!(serde_json::from_str::<World>(&json)
        .unwrap()
        .exports
        .iter()
        .flat_map(|i| &i.functions)
        .all(|f| !f.async_));
}

#[test]
fn p3_http_host_binding_keeps_exact_handler_version_and_async_exports() {
    for version in ["0.3.0", "0.3.1"] {
        let world=parse(&format!("package wasi:http@{version}; interface handler {{ handle: async func(); }} world proxy {{ export handler; }}"), None).unwrap();
        let host = typescript(&world);
        assert_eq!(
            host.contains("handleHttp(server: ComponentHttpServer): Promise<void>"),
            version == "0.3.0"
        );
        assert!(!typescript_guest(&world).contains("handleComponentHttpRequest"));
        if version == "0.3.0" {
            assert!(host.contains(
                "handleComponentHttpRequest(server, handle, \"wasi:http/handler@0.3.0\")"
            ));
        }
    }
}

#[test]
fn character_streams_are_rejected_before_emitting_unloadable_bindings() {
    for payload in ["char", "letter"] {
        let source = format!("package test:chars; interface data {{ type letter = char; get: func() -> stream<{payload}>; }} world plugin {{ export data; }}");
        let error = parse(&source, None).unwrap_err();
        assert!(
            format!("{error:#}").contains("stream<char> is unsupported"),
            "{error:#}"
        );
    }
    assert!(parse(
        "package test:chars; world plugin { export get: func() -> future<char>; }",
        None
    )
    .is_ok());
}

#[cfg(unix)]
#[test]
fn experimental_async_exports_compile_without_changing_the_public_contract() {
    for source in [
        "package test:async-export; world plugin { export run: async func(id: u32) -> u32; export wake: func(id: u32); export perry-scheduler-signal-0: func(); }",
        "package test:async-export; interface api { record data { text: string, bytes: list<u8> } run: async func(input: data) -> result<data, string>; sync: func() -> data; empty: async func(); } world plugin { import log: func(text: string); export api; }",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("world.wit");
        std::fs::write(&input, source).unwrap();
        assert!(canonical_guest(&input, None).unwrap_err().to_string().contains("async guest functions"));
        let world = parse(source, None).unwrap();
        let files = experimental_async_guest(&input, None).unwrap();
        let adapter = String::from_utf8(files["perry_adapter.c"].clone()).unwrap();
        let contract = serde_json::to_vec(&world).unwrap().iter().map(u8::to_string).collect::<Vec<_>>().join(",");
        assert!(adapter.contains(&format!("perry_contract[] = {{{contract}}}")));
        assert!(adapter.contains("perry_component_frame_task_checkpoint"));
        assert!(!adapter.contains("perry_component_frame_call(f,"));
        assert!(!adapter.contains("CALLBACK_CODE_YIELD"));
        for (name, bytes) in files { std::fs::write(dir.path().join(name), bytes).unwrap(); }
        let output = std::process::Command::new("clang").args(["-std=c11", "-Wall", "-Werror", "-Wno-unused-function", "-fsyntax-only"]).arg(dir.path().join("perry_adapter.c")).output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    }
}

#[test]
fn experimental_async_exports_reject_unqualified_async_values() {
    for (source, expected) in [
        ("package test:profile; world plugin { export run: async func() -> future<u32>; }", "future/stream"),
        ("package test:profile; world plugin { export run: async func() -> stream<u32>; }", "future/stream"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("world.wit");
        std::fs::write(&input, source).unwrap();
        assert!(experimental_async_guest(&input, None).unwrap_err().to_string().contains(expected));
    }
}

#[cfg(unix)]
#[test]
fn experimental_async_imports_compile_with_the_backends_indirect_and_flat_signatures() {
    for source in [
        "package test:imports; world plugin { import empty: async func(); import scalar: async func(a: u32, b: u64, c: bool) -> u32; import text: async func(input: string) -> string; export run: async func() -> u32; }",
        "package test:imports; interface api { record payload { text: string, bytes: list<u8>, labels: option<list<string>> } single: async func(input: payload) -> payload; many: async func(a: payload, b: payload, c: payload, d: payload, e: payload) -> result<payload, string>; } world plugin { import api; export run: async func(); }",
        "package test:imports; world plugin { import unused: async func(); export sync: func() -> u32; }",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("world.wit");
        std::fs::write(&input, source).unwrap();
        assert!(canonical_guest(&input, None).is_err());
        let files = experimental_async_guest(&input, None).unwrap();
        let adapter = String::from_utf8(files["perry_adapter.c"].clone()).unwrap();
        assert!(adapter.contains("Async import requires an async export task"));
        assert!(adapter.contains("perry_component_frame_import_begin(f)"));
        assert!(adapter.contains("perry_component_frame_import_resolve(f,"));
        assert!(!adapter.contains("CALLBACK_CODE_YIELD"));
        for (name, bytes) in files { std::fs::write(dir.path().join(name), bytes).unwrap(); }
        let output = std::process::Command::new("clang").args(["-std=c11", "-Wall", "-Werror", "-Wno-unused-function", "-fsyntax-only"]).arg(dir.path().join("perry_adapter.c")).output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    }
}
