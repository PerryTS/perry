use super::*;

/// Issue #2531 — a symlinked `perry` (the `cargo`/dev-install default,
/// e.g. `~/.cargo/bin/perry -> .../target/release/perry`) must still
/// locate the workspace root. Without canonicalizing the exe first, the
/// `../../` walk climbs the symlink's own ancestors and misses the
/// workspace, silently dropping the per-feature `perry-ext-*` libs.
#[cfg(unix)]
mod workspace_root_symlink_tests {
    use super::super::workspace_root_from_exe;

    #[test]
    fn symlinked_exe_resolves_to_workspace_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        // Lay out a fake workspace: <root>/target/release/perry plus the
        // two marker crates the detector keys off of.
        let real_dir = root.join("target/release");
        std::fs::create_dir_all(&real_dir).expect("mkdir target/release");
        let real_exe = real_dir.join("perry");
        std::fs::write(&real_exe, b"#!/bin/true\n").expect("write exe");
        std::fs::create_dir_all(root.join("crates/perry-runtime")).expect("mkdir runtime");
        std::fs::create_dir_all(root.join("crates/perry-ui-geisterhand")).expect("mkdir gh");

        // Install perry as a symlink under a sibling "bin" dir whose
        // ancestors contain no workspace markers (mimics ~/.cargo/bin).
        let bin_dir = root.join("bin");
        std::fs::create_dir_all(&bin_dir).expect("mkdir bin");
        let link = bin_dir.join("perry");
        std::os::unix::fs::symlink(&real_exe, &link).expect("symlink");

        let found = workspace_root_from_exe(&link).expect("workspace root via symlink");
        assert_eq!(
            found,
            std::fs::canonicalize(root).expect("canonicalize root")
        );
    }

    #[test]
    fn missing_ancestor_does_not_abort_before_match() {
        // The real binary lives at <root>/target/release/perry; the
        // deeper `../../../..` ancestor may climb above the tempdir but
        // must not bail before the `../..` match at the workspace root.
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let real_dir = root.join("target/release");
        std::fs::create_dir_all(&real_dir).expect("mkdir target/release");
        let real_exe = real_dir.join("perry");
        std::fs::write(&real_exe, b"#!/bin/true\n").expect("write exe");
        std::fs::create_dir_all(root.join("crates/perry-runtime")).expect("mkdir runtime");
        std::fs::create_dir_all(root.join("crates/perry-ui-geisterhand")).expect("mkdir gh");

        let found = workspace_root_from_exe(&real_exe).expect("workspace root");
        assert_eq!(
            found,
            std::fs::canonicalize(root).expect("canonicalize root")
        );
    }
}

#[cfg(test)]
mod abi_validation_tests {
    use super::*;

    fn manifest_with_abi(abi: Option<&str>) -> NativeLibraryManifest {
        NativeLibraryManifest {
            module: "test".to_string(),
            package_dir: PathBuf::new(),
            abi_version: abi.map(String::from),
            functions: vec![],
            target_config: None,
        }
    }

    #[test]
    fn missing_abi_version_warns_but_passes() {
        let m = manifest_with_abi(None);
        assert!(validate_abi_version(&m).is_ok());
    }

    #[test]
    fn matching_caret_range_passes() {
        // The bundled version is whatever this build is compiled
        // against — by definition the same major.minor as itself.
        let v = PERRY_FFI_ABI_VERSION;
        let major_minor = v.splitn(3, '.').take(2).collect::<Vec<_>>().join(".");
        let m = manifest_with_abi(Some(&major_minor));
        assert!(
            validate_abi_version(&m).is_ok(),
            "wrapper declaring `{}` should validate against bundled `{}`",
            major_minor,
            v
        );
    }

    #[test]
    fn future_major_fails() {
        // `^99.0` rejects every actual perry-ffi version that ships
        // this decade. Use `^99` so we don't need to bump the test
        // when the runtime hits a multi-digit minor.
        let m = manifest_with_abi(Some("99"));
        let err = validate_abi_version(&m).expect_err("99 must reject current ABI");
        assert!(err.contains("perry-ffi"), "got: {}", err);
        assert!(err.contains("test"), "got: {}", err);
    }

    #[test]
    fn unparseable_abi_version_returns_error() {
        let m = manifest_with_abi(Some("not a version"));
        let err = validate_abi_version(&m).expect_err("garbage must reject");
        assert!(err.contains("unparseable"), "got: {}", err);
    }
}

#[cfg(test)]
mod manifest_parse_tests {
    use super::*;
    use perry_api_manifest::{NativeAbiType, NativeHandleOwnership, NativeHandleThreadAffinity};

    fn parse_manifest_from_functions(
        pkg_dir: &Path,
        functions: serde_json::Value,
    ) -> Result<Option<NativeLibraryManifest>> {
        let manifest = serde_json::json!({
            "perry": {
                "nativeLibrary": {
                    "functions": functions,
                    "targets": { "macos": { "crate": "rust", "lib": "demo" } }
                }
            }
        });
        std::fs::write(
            pkg_dir.join("package.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .expect("write package.json");
        parse_native_library_manifest(pkg_dir, "demo", Some("macos"))
    }

    fn parse_manifest_error(function: serde_json::Value) -> String {
        let dir = tempfile::tempdir().expect("tempdir");
        let err = parse_manifest_from_functions(dir.path(), serde_json::json!([function]))
            .expect_err("manifest must be rejected");
        err.to_string()
    }

    #[test]
    fn native_abi_descriptors_parse_strings_and_structured_forms() {
        let dir = tempfile::tempdir().expect("tempdir");
        let parsed = parse_manifest_from_functions(
            dir.path(),
            serde_json::json!([
                {
                    "name": "all_descriptors",
                    "params": [
                        "jsvalue",
                        "string",
                        "bool",
                        "i32",
                        "i64",
                        "u32",
                        "u64",
                        "usize",
                        "f32",
                        "f64",
                        "number",
                        "ptr",
                        "buffer_len",
                        "buffer+len",
                        "handle",
                        "promise",
                        { "kind": "handle", "type": "MyThing" },
                        {
                            "kind": "handle",
                            "type": "SharedThing",
                            "ownership": "owned",
                            "nullable": true,
                            "thread": "creator",
                            "debugName": "SharedThingHandle"
                        },
                        { "kind": "promise", "result": "jsvalue" },
                        {
                            "kind": "pod",
                            "name": "Packet",
                            "fields": [
                                { "name": "tag", "type": "u32" },
                                { "name": "gain", "type": "f32" },
                                { "name": "total", "type": "number" },
                                { "name": "count", "type": "buffer_len" }
                            ]
                        },
                        {
                            "kind": "pod+count",
                            "name": "PacketBatch",
                            "fields": [
                                { "name": "tag", "type": "u32" },
                                { "name": "owner", "type": "handle_id" },
                                {
                                    "name": "meta",
                                    "abi": {
                                        "kind": "pod",
                                        "name": "PacketMeta",
                                        "fields": [
                                            { "name": "seq", "type": "u64" }
                                        ]
                                    }
                                }
                            ]
                        },
                        { "kind": "buffer+len" }
                    ],
                    "returns": { "kind": "promise", "result": "f64" }
                },
                {
                    "name": "void_return",
                    "params": [],
                    "returns": "void"
                }
            ]),
        )
        .expect("parse manifest")
        .expect("manifest");

        let function = &parsed.functions[0];
        let displays: Vec<String> = function.params.iter().map(ToString::to_string).collect();
        assert_eq!(
            displays,
            vec![
                "jsvalue",
                "string",
                "bool",
                "i32",
                "i64",
                "u32",
                "u64",
                "usize",
                "f32",
                "f64",
                "f64",
                "ptr",
                "buffer_len",
                "buffer+len",
                "handle",
                "promise<jsvalue>",
                "handle<MyThing>",
                "handle<SharedThing>",
                "promise<jsvalue>",
                "pod<Packet>",
                "pod+count<PacketBatch>",
                "buffer+len",
            ]
        );
        match &function.params[17] {
            NativeAbiType::Handle(handle) => {
                assert_eq!(handle.type_name.as_deref(), Some("SharedThing"));
                assert_eq!(handle.ownership, NativeHandleOwnership::Owned);
                assert!(handle.nullable);
                assert_eq!(handle.thread, NativeHandleThreadAffinity::Creator);
                assert_eq!(handle.finalizer, None);
                assert_eq!(handle.debug_name, "SharedThingHandle");
            }
            other => panic!("expected handle descriptor, got {other:?}"),
        }
        match &function.params[20] {
            NativeAbiType::PodAndCount(pod) => {
                assert_eq!(pod.name.as_deref(), Some("PacketBatch"));
                assert_eq!(pod.fields.len(), 3);
                assert!(matches!(pod.fields[1].ty, NativeAbiType::HandleId));
                assert!(matches!(pod.fields[2].ty, NativeAbiType::Pod(_)));
            }
            other => panic!("expected pod+count descriptor, got {other:?}"),
        }
        assert_eq!(function.returns.to_string(), "promise<f64>");
        assert!(matches!(parsed.functions[1].returns, NativeAbiType::Void));
    }

    #[test]
    fn native_async_promise_return_parses_completion_metadata() {
        let dir = tempfile::tempdir().expect("tempdir");
        let parsed = parse_manifest_from_functions(
            dir.path(),
            serde_json::json!([
                {
                    "name": "native_async_return",
                    "params": [],
                    "returns": {
                        "kind": "promise",
                        "result": "f64",
                        "completion": "native_async",
                        "thread": "main"
                    }
                }
            ]),
        )
        .expect("parse manifest")
        .expect("manifest");

        let ret = &parsed.functions[0].returns;
        assert_eq!(ret.to_string(), "promise<f64>");
        assert_eq!(
            ret.promise_completion()
                .map(|completion| completion.as_str()),
            Some("native_async")
        );
        assert_eq!(
            ret.promise_thread().map(|thread| thread.as_str()),
            Some("main")
        );
        assert_eq!(
            ret.promise_result().map(ToString::to_string),
            Some("f64".into())
        );
    }

    #[test]
    fn native_async_promise_param_is_rejected() {
        let err = parse_manifest_error(serde_json::json!({
            "name": "bad_native_async_param",
            "params": [
                {
                    "kind": "promise",
                    "result": "jsvalue",
                    "completion": "native_async"
                }
            ],
            "returns": "void"
        }));
        assert!(err.contains("bad_native_async_param"), "{err}");
        assert!(err.contains("params[0]"), "{err}");
        assert!(err.contains("native_async"), "{err}");
        assert!(err.contains("valid only on returns"), "{err}");
    }

    #[test]
    fn native_async_promise_rejects_invalid_completion_and_thread_fields() {
        let completion_err = parse_manifest_error(serde_json::json!({
            "name": "bad_promise_completion",
            "params": [],
            "returns": {
                "kind": "promise",
                "completion": "later"
            }
        }));
        assert!(completion_err.contains("completion"), "{completion_err}");
        assert!(completion_err.contains("direct"), "{completion_err}");

        let thread_err = parse_manifest_error(serde_json::json!({
            "name": "bad_promise_thread",
            "params": [],
            "returns": {
                "kind": "promise",
                "thread": "worker"
            }
        }));
        assert!(thread_err.contains("thread"), "{thread_err}");
        assert!(thread_err.contains("main"), "{thread_err}");
    }

    #[test]
    fn native_abi_owned_return_handle_parses_finalizer_contract() {
        let dir = tempfile::tempdir().expect("tempdir");
        let parsed = parse_manifest_from_functions(
            dir.path(),
            serde_json::json!([
                {
                    "name": "make_handle",
                    "params": [],
                    "returns": {
                        "kind": "handle",
                        "type": "OwnedThing",
                        "ownership": "owned",
                        "nullable": false,
                        "thread": "main",
                        "finalizer": "owned_thing_free",
                        "debugName": "OwnedThing"
                    }
                }
            ]),
        )
        .expect("parse manifest")
        .expect("manifest");

        match &parsed.functions[0].returns {
            NativeAbiType::Handle(handle) => {
                assert_eq!(handle.type_name.as_deref(), Some("OwnedThing"));
                assert_eq!(handle.ownership, NativeHandleOwnership::Owned);
                assert_eq!(handle.thread, NativeHandleThreadAffinity::Main);
                assert_eq!(handle.finalizer.as_deref(), Some("owned_thing_free"));
                assert_eq!(handle.debug_name, "OwnedThing");
            }
            other => panic!("expected handle return, got {other:?}"),
        }
    }

    #[test]
    fn native_abi_unknown_type_reports_package_function_slot_and_spelling() {
        let err = parse_manifest_error(serde_json::json!({
            "name": "bad_unknown",
            "params": ["bogus"],
            "returns": "void"
        }));
        assert!(err.contains("package.json"), "{err}");
        assert!(err.contains("functions[0]"), "{err}");
        assert!(err.contains("bad_unknown"), "{err}");
        assert!(err.contains("params[0]"), "{err}");
        assert!(err.contains("bogus"), "{err}");
    }

    #[test]
    fn native_abi_void_param_is_rejected() {
        let err = parse_manifest_error(serde_json::json!({
            "name": "bad_void",
            "params": ["void"],
            "returns": "void"
        }));
        assert!(err.contains("package.json"), "{err}");
        assert!(err.contains("bad_void"), "{err}");
        assert!(err.contains("params[0]"), "{err}");
        assert!(err.contains("void"), "{err}");
    }

    #[test]
    fn native_abi_buffer_and_len_return_is_rejected() {
        let err = parse_manifest_error(serde_json::json!({
            "name": "bad_return",
            "params": [],
            "returns": "buffer+len"
        }));
        assert!(err.contains("package.json"), "{err}");
        assert!(err.contains("bad_return"), "{err}");
        assert!(err.contains("returns"), "{err}");
        assert!(err.contains("buffer+len"), "{err}");
    }

    #[test]
    fn native_abi_pod_and_count_return_is_rejected() {
        let err = parse_manifest_error(serde_json::json!({
            "name": "bad_pod_view_return",
            "params": [],
            "returns": {
                "kind": "pod+count",
                "name": "PacketBatch",
                "fields": [{ "name": "tag", "type": "u32" }]
            }
        }));
        assert!(err.contains("package.json"), "{err}");
        assert!(err.contains("bad_pod_view_return"), "{err}");
        assert!(err.contains("returns"), "{err}");
        assert!(err.contains("pod+count"), "{err}");
    }

    #[test]
    fn native_abi_handle_id_is_valid_only_inside_pod_fields() {
        let err = parse_manifest_error(serde_json::json!({
            "name": "bad_handle_id_param",
            "params": ["handle_id"],
            "returns": "void"
        }));
        assert!(err.contains("package.json"), "{err}");
        assert!(err.contains("bad_handle_id_param"), "{err}");
        assert!(err.contains("params[0]"), "{err}");
        assert!(err.contains("handle_id"), "{err}");
    }

    #[test]
    fn native_abi_manifest_pod_empty_fields_are_rejected() {
        let err = parse_manifest_error(serde_json::json!({
            "name": "bad_pod",
            "params": [{ "kind": "pod", "fields": [] }],
            "returns": "void"
        }));
        assert!(err.contains("package.json"), "{err}");
        assert!(err.contains("bad_pod"), "{err}");
        assert!(err.contains("fields"), "{err}");
    }

    #[test]
    fn native_abi_manifest_nested_pod_empty_fields_are_rejected() {
        let err = parse_manifest_error(serde_json::json!({
            "name": "bad_nested_pod",
            "params": [{
                "kind": "pod+count",
                "name": "PacketBatch",
                "fields": [{
                    "name": "meta",
                    "abi": { "kind": "pod", "name": "Meta", "fields": [] }
                }]
            }],
            "returns": "void"
        }));
        assert!(err.contains("package.json"), "{err}");
        assert!(err.contains("bad_nested_pod"), "{err}");
        assert!(err.contains("params[0].fields[0].type"), "{err}");
        assert!(err.contains("at least one field"), "{err}");
    }

    #[test]
    fn native_abi_manifest_pod_rejects_pointer_fields() {
        let err = parse_manifest_error(serde_json::json!({
            "name": "bad_pod_field",
            "params": [{
                "kind": "pod",
                "fields": [
                    { "name": "ptr", "type": "handle" }
                ]
            }],
            "returns": "void"
        }));
        assert!(err.contains("package.json"), "{err}");
        assert!(err.contains("bad_pod_field"), "{err}");
        assert!(err.contains("fields[0].type"), "{err}");
        assert!(
            err.contains("POD") || err.contains("pod field type"),
            "{err}"
        );
    }

    #[test]
    fn native_abi_malformed_handle_string_is_rejected() {
        let err = parse_manifest_error(serde_json::json!({
            "name": "bad_handle",
            "params": ["handle<>"],
            "returns": "void"
        }));
        assert!(err.contains("package.json"), "{err}");
        assert!(err.contains("bad_handle"), "{err}");
        assert!(err.contains("params[0]"), "{err}");
        assert!(err.contains("handle<>"), "{err}");
    }

    #[test]
    fn native_abi_malformed_structured_object_is_rejected() {
        let err = parse_manifest_error(serde_json::json!({
            "name": "bad_structured",
            "params": [{ "kind": "handle", "type": 7 }],
            "returns": "void"
        }));
        assert!(err.contains("package.json"), "{err}");
        assert!(err.contains("bad_structured"), "{err}");
        assert!(err.contains("params[0]"), "{err}");
        assert!(err.contains("invalid ABI"), "{err}");
        assert!(err.contains("handle"), "{err}");
    }

    #[test]
    fn native_abi_handle_unknown_field_is_rejected() {
        let err = parse_manifest_error(serde_json::json!({
            "name": "bad_handle_field",
            "params": [{ "kind": "handle", "type": "Thing", "surprise": true }],
            "returns": "void"
        }));
        assert!(err.contains("package.json"), "{err}");
        assert!(err.contains("bad_handle_field"), "{err}");
        assert!(err.contains("params[0]"), "{err}");
        assert!(err.contains("surprise"), "{err}");
    }

    #[test]
    fn native_abi_handle_invalid_enums_are_rejected() {
        let ownership_err = parse_manifest_error(serde_json::json!({
            "name": "bad_handle_ownership",
            "params": [{ "kind": "handle", "ownership": "shared" }],
            "returns": "void"
        }));
        assert!(ownership_err.contains("ownership"), "{ownership_err}");
        assert!(ownership_err.contains("shared"), "{ownership_err}");

        let thread_err = parse_manifest_error(serde_json::json!({
            "name": "bad_handle_thread",
            "params": [{ "kind": "handle", "thread": "worker" }],
            "returns": "void"
        }));
        assert!(thread_err.contains("thread"), "{thread_err}");
        assert!(thread_err.contains("worker"), "{thread_err}");
    }

    #[test]
    fn native_abi_handle_finalizer_requires_owned_return() {
        let borrowed_err = parse_manifest_error(serde_json::json!({
            "name": "bad_borrowed_finalizer",
            "params": [],
            "returns": { "kind": "handle", "finalizer": "free_thing" }
        }));
        assert!(
            borrowed_err.contains("bad_borrowed_finalizer"),
            "{borrowed_err}"
        );
        assert!(borrowed_err.contains("ownership"), "{borrowed_err}");

        let param_err = parse_manifest_error(serde_json::json!({
            "name": "bad_param_finalizer",
            "params": [{
                "kind": "handle",
                "ownership": "owned",
                "finalizer": "free_thing"
            }],
            "returns": "void"
        }));
        assert!(param_err.contains("bad_param_finalizer"), "{param_err}");
        assert!(param_err.contains("params[0]"), "{param_err}");
        assert!(param_err.contains("returns"), "{param_err}");
    }

    /// Relative `libDirs` entries must resolve against the package's
    /// own directory, not the user's cwd — otherwise a wrapper that
    /// ships a `vendor/lib/` alongside its `package.json` would only
    /// link when invoked from one specific directory. Absolute entries
    /// pass through unchanged (`PathBuf::join` ignores the base when
    /// the right-hand side is absolute).
    #[test]
    fn lib_dirs_relative_paths_anchored_to_package_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pkg_dir = dir.path();
        let manifest = serde_json::json!({
            "perry": {
                "nativeLibrary": {
                    "functions": [],
                    "targets": {
                        "macos": {
                            "crate": "rust",
                            "lib": "demo",
                            "libDirs": ["vendor/lib", "/abs/path"]
                        }
                    }
                }
            }
        });
        std::fs::write(
            pkg_dir.join("package.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .expect("write package.json");

        let parsed = parse_native_library_manifest(pkg_dir, "demo", Some("macos"))
            .expect("parse manifest")
            .expect("parsed manifest");
        let tc = parsed.target_config.expect("target_config");
        assert_eq!(tc.lib_dirs.len(), 2);
        assert_eq!(tc.lib_dirs[0], pkg_dir.join("vendor/lib"));
        assert_eq!(tc.lib_dirs[1], PathBuf::from("/abs/path"));
    }

    /// Omitted `libDirs` must default to an empty list, not error —
    /// it's an optional field on every existing wrapper.
    #[test]
    fn lib_dirs_defaults_to_empty_when_absent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pkg_dir = dir.path();
        let manifest = serde_json::json!({
            "perry": {
                "nativeLibrary": {
                    "functions": [],
                    "targets": { "macos": { "crate": "rust", "lib": "demo" } }
                }
            }
        });
        std::fs::write(
            pkg_dir.join("package.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .expect("write package.json");

        let parsed = parse_native_library_manifest(pkg_dir, "demo", Some("macos"))
            .expect("parse manifest")
            .expect("parsed manifest");
        let tc = parsed.target_config.expect("target_config");
        assert!(tc.lib_dirs.is_empty());
    }

    /// Issue #860 — `targets.<os>-<arch>` keys take precedence over
    /// the bare `targets.<os>` key. A wrapper that ships per-arch
    /// prebuilts (esbuild/sharp/swc pattern) needs to direct macos
    /// arm64 vs macos x64 consumers at different `.a` archives even
    /// though both pass `--target macos`.
    #[test]
    fn per_arch_target_key_beats_bare_os_key() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pkg_dir = dir.path();
        let manifest = serde_json::json!({
            "perry": {
                "nativeLibrary": {
                    "functions": [],
                    "targets": {
                        "macos":       { "crate": "rust",       "lib": "fallback" },
                        "macos-arm64": { "crate": "rust-arm64", "lib": "arm64_lib" },
                        "macos-x64":   { "crate": "rust-x64",   "lib": "x64_lib"   }
                    }
                }
            }
        });
        std::fs::write(
            pkg_dir.join("package.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .expect("write package.json");

        // The arch key for `Some("macos")` is hard-coded to `arm64`
        // by `arch_for_target_key` — that's the production macOS
        // distribution arch (Apple Silicon). x64 entries can still be
        // delivered by passing a different target string in the
        // future; we just need the per-arch lookup to fire.
        let parsed = parse_native_library_manifest(pkg_dir, "demo", Some("macos"))
            .expect("parse manifest")
            .expect("parsed manifest");
        let tc = parsed.target_config.expect("target_config");
        assert_eq!(tc.lib_name, "arm64_lib");
        assert_eq!(tc.crate_path, pkg_dir.join("rust-arm64"));
    }

    /// When no per-arch key matches, the bare OS-only key still
    /// resolves — existing on-disk wrappers must not regress.
    #[test]
    fn falls_back_to_bare_os_key_when_per_arch_absent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pkg_dir = dir.path();
        let manifest = serde_json::json!({
            "perry": {
                "nativeLibrary": {
                    "functions": [],
                    "targets": {
                        "macos": { "crate": "rust", "lib": "demo" }
                    }
                }
            }
        });
        std::fs::write(
            pkg_dir.join("package.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .expect("write package.json");

        let parsed = parse_native_library_manifest(pkg_dir, "demo", Some("macos"))
            .expect("parse manifest")
            .expect("parsed manifest");
        let tc = parsed.target_config.expect("target_config");
        assert_eq!(tc.lib_name, "demo");
        assert!(tc.prebuilt.is_none());
    }

    /// Issue #860 — `prebuilt:` pointing at a node-style module
    /// reference (`@scope/pkg/subpath/file.a`) resolves through the
    /// consumer's `node_modules`. This is the esbuild/sharp/swc
    /// distribution shape: a thin meta-package declares optional
    /// per-platform subpackages via `optionalDependencies`, npm
    /// installs only the matching one, and `prebuilt:` reaches into
    /// it without invoking cargo.
    #[test]
    fn prebuilt_resolves_node_modules_subpackage() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        // Lay out a realistic node_modules: consumer/node_modules/
        // @bloomengine/{engine, engine-darwin-arm64}/.
        let consumer_pkg = root
            .join("node_modules")
            .join("@bloomengine")
            .join("engine");
        let prebuilt_pkg = root
            .join("node_modules")
            .join("@bloomengine")
            .join("engine-darwin-arm64")
            .join("lib");
        std::fs::create_dir_all(&consumer_pkg).expect("mkdir engine");
        std::fs::create_dir_all(&prebuilt_pkg).expect("mkdir engine-darwin-arm64/lib");
        let prebuilt_file = prebuilt_pkg.join("libbloom_macos.a");
        std::fs::write(&prebuilt_file, b"fake archive").expect("write prebuilt");

        let manifest = serde_json::json!({
            "perry": {
                "nativeLibrary": {
                    "functions": [],
                    "targets": {
                        "macos-arm64": {
                            "prebuilt": "@bloomengine/engine-darwin-arm64/lib/libbloom_macos.a",
                            "frameworks": ["Metal", "QuartzCore"]
                        }
                    }
                }
            }
        });
        std::fs::write(
            consumer_pkg.join("package.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .expect("write engine/package.json");

        let parsed =
            parse_native_library_manifest(&consumer_pkg, "@bloomengine/engine", Some("macos"))
                .expect("parse manifest")
                .expect("parsed manifest");
        let tc = parsed.target_config.expect("target_config");
        let prebuilt = tc.prebuilt.expect("prebuilt path");
        // Use canonicalize on both sides — the test's `tmpdir` on
        // macOS lives under `/var/...` which is a symlink to
        // `/private/var/...`; the resolver returns the symlinked
        // form, the original `prebuilt_file` was constructed with
        // the symlinked form too, so they match before canonicalize
        // here. But canonicalize defensively in case CI tmpdirs differ.
        assert_eq!(
            prebuilt.canonicalize().expect("canonicalize prebuilt"),
            prebuilt_file.canonicalize().expect("canonicalize expected")
        );
        assert_eq!(tc.frameworks, vec!["Metal", "QuartzCore"]);
        // The cargo build path should still be empty — no `crate:`
        // means the prebuilt branch is exclusive.
        assert_eq!(tc.lib_name, "");
    }

    /// Relative `prebuilt:` paths anchor against the package's own
    /// directory — useful for tarball-shipped wrappers that vendor
    /// the static lib alongside their `package.json`.
    #[test]
    fn prebuilt_relative_path_anchors_to_package_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pkg_dir = dir.path();
        let vendor_dir = pkg_dir.join("vendor");
        std::fs::create_dir_all(&vendor_dir).expect("mkdir vendor");
        let lib_path = vendor_dir.join("libfoo.a");
        std::fs::write(&lib_path, b"fake archive").expect("write lib");

        let manifest = serde_json::json!({
            "perry": {
                "nativeLibrary": {
                    "functions": [],
                    "targets": {
                        "macos": { "prebuilt": "./vendor/libfoo.a" }
                    }
                }
            }
        });
        std::fs::write(
            pkg_dir.join("package.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .expect("write package.json");

        let parsed = parse_native_library_manifest(pkg_dir, "demo", Some("macos"))
            .expect("parse manifest")
            .expect("parsed manifest");
        let tc = parsed.target_config.expect("target_config");
        let prebuilt = tc.prebuilt.expect("prebuilt path");
        assert_eq!(prebuilt, pkg_dir.join("./vendor/libfoo.a"));
    }

    /// Issue #1304 — vendored-SDK frameworks parse from the snake_case
    /// manifest keys (`optional_frameworks` / `frameworks_env`), matching
    /// the `swift_sources` / `metal_sources` convention. These are the
    /// shape `@perryts/google-auth` uses for the real GoogleSignIn SDK.
    #[test]
    fn optional_frameworks_parse_snake_case() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pkg_dir = dir.path();
        let manifest = serde_json::json!({
            "perry": {
                "nativeLibrary": {
                    "functions": [],
                    "targets": {
                        "ios": {
                            "crate": "crate-ios",
                            "lib": "perry_google_auth",
                            "optional_frameworks": ["GoogleSignIn"],
                            "frameworks_env": "PERRY_GOOGLE_SIGN_IN_FRAMEWORK_DIR"
                        }
                    }
                }
            }
        });
        std::fs::write(
            pkg_dir.join("package.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .expect("write package.json");

        let parsed = parse_native_library_manifest(pkg_dir, "demo", Some("ios"))
            .expect("parse manifest")
            .expect("parsed manifest");
        let tc = parsed.target_config.expect("target_config");
        assert_eq!(tc.optional_frameworks, vec!["GoogleSignIn"]);
        assert_eq!(
            tc.frameworks_env.as_deref(),
            Some("PERRY_GOOGLE_SIGN_IN_FRAMEWORK_DIR")
        );
    }

    /// The camelCase spelling (`optionalFrameworks` / `frameworksEnv`,
    /// matching `libDirs` / `pkgConfig`) is accepted too — the manifest's
    /// casing convention is mixed, so we don't want a silent no-op when an
    /// author picks the camelCase form.
    #[test]
    fn optional_frameworks_parse_camel_case() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pkg_dir = dir.path();
        let manifest = serde_json::json!({
            "perry": {
                "nativeLibrary": {
                    "functions": [],
                    "targets": {
                        "ios": {
                            "crate": "crate-ios",
                            "lib": "demo",
                            "optionalFrameworks": ["GoogleSignIn", "AppAuth"],
                            "frameworksEnv": "VENDOR_FW_DIR"
                        }
                    }
                }
            }
        });
        std::fs::write(
            pkg_dir.join("package.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .expect("write package.json");

        let parsed = parse_native_library_manifest(pkg_dir, "demo", Some("ios"))
            .expect("parse manifest")
            .expect("parsed manifest");
        let tc = parsed.target_config.expect("target_config");
        assert_eq!(tc.optional_frameworks, vec!["GoogleSignIn", "AppAuth"]);
        assert_eq!(tc.frameworks_env.as_deref(), Some("VENDOR_FW_DIR"));
    }

    /// Omitting both fields must default to an empty list / `None`, not
    /// error — every existing wrapper lacks them.
    #[test]
    fn optional_frameworks_default_when_absent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pkg_dir = dir.path();
        let manifest = serde_json::json!({
            "perry": {
                "nativeLibrary": {
                    "functions": [],
                    "targets": { "ios": { "crate": "rust", "lib": "demo" } }
                }
            }
        });
        std::fs::write(
            pkg_dir.join("package.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .expect("write package.json");

        let parsed = parse_native_library_manifest(pkg_dir, "demo", Some("ios"))
            .expect("parse manifest")
            .expect("parsed manifest");
        let tc = parsed.target_config.expect("target_config");
        assert!(tc.optional_frameworks.is_empty());
        assert!(tc.frameworks_env.is_none());
    }

    #[test]
    fn parses_metal_backend_target_metadata() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pkg_dir = dir.path();
        let manifest = serde_json::json!({
            "perry": {
                "nativeLibrary": {
                    "functions": [],
                    "targets": {
                        "ios": {
                            "crate": "crate-ios",
                            "lib": "demo",
                            "backends": {
                                "metal": {
                                    "frameworks": ["Metal", "QuartzCore"],
                                    "shaderSources": ["shaders/default.metal"],
                                    "shaderOutputs": ["prebuilt/default.metallib"],
                                    "resources": ["resources/metal"],
                                    "package": {
                                        "name": "demo-metal",
                                        "version": "1.0.0",
                                        "kind": "metallib"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });
        std::fs::write(
            pkg_dir.join("package.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .expect("write package.json");

        let parsed = parse_native_library_manifest(pkg_dir, "demo", Some("ios"))
            .expect("parse manifest")
            .expect("parsed manifest");
        let tc = parsed.target_config.expect("target_config");
        let backend = tc.backends.first().expect("metal backend");
        assert_eq!(backend.backend, NativeBackend::Metal);
        assert_eq!(backend.frameworks, vec!["Metal", "QuartzCore"]);
        assert_eq!(
            backend.shader_sources,
            vec![pkg_dir.join("shaders/default.metal")]
        );
        assert_eq!(
            backend.shader_outputs,
            vec![pkg_dir.join("prebuilt/default.metallib")]
        );
        assert_eq!(backend.resources, vec![pkg_dir.join("resources/metal")]);
        assert_eq!(backend.package.name.as_deref(), Some("demo-metal"));
        assert_eq!(backend.package.kind.as_deref(), Some("metallib"));
    }

    #[test]
    fn parses_vulkan_backend_target_metadata() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pkg_dir = dir.path();
        let manifest = serde_json::json!({
            "perry": {
                "nativeLibrary": {
                    "functions": [],
                    "targets": {
                        "linux": {
                            "crate": "crate-linux",
                            "lib": "demo",
                            "backends": {
                                "vulkan": {
                                    "libs": ["vulkan"],
                                    "libDirs": ["vendor/vulkan/lib"],
                                    "shaderOutputs": ["shaders/default.spv"]
                                }
                            }
                        }
                    }
                }
            }
        });
        std::fs::write(
            pkg_dir.join("package.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .expect("write package.json");

        let parsed = parse_native_library_manifest(pkg_dir, "demo", Some("linux"))
            .expect("parse manifest")
            .expect("parsed manifest");
        let tc = parsed.target_config.expect("target_config");
        let backend = tc.backends.first().expect("vulkan backend");
        assert_eq!(backend.backend, NativeBackend::Vulkan);
        assert_eq!(backend.libs, vec!["vulkan"]);
        assert_eq!(backend.lib_dirs, vec![pkg_dir.join("vendor/vulkan/lib")]);
        assert_eq!(
            backend.shader_outputs,
            vec![pkg_dir.join("shaders/default.spv")]
        );
    }

    #[test]
    fn parses_d3d12_backend_target_metadata() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pkg_dir = dir.path();
        let manifest = serde_json::json!({
            "perry": {
                "nativeLibrary": {
                    "functions": [],
                    "targets": {
                        "windows": {
                            "prebuilt": "./prebuilt/demo.lib",
                            "backends": {
                                "d3d12": {
                                    "libs": ["d3d12", "dxgi", "dxguid"],
                                    "shaderSources": ["shaders/default.hlsl"],
                                    "shaderOutputs": ["shaders/default.dxil"]
                                }
                            }
                        }
                    }
                }
            }
        });
        std::fs::write(
            pkg_dir.join("package.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .expect("write package.json");

        let parsed = parse_native_library_manifest(pkg_dir, "demo", Some("windows"))
            .expect("parse manifest")
            .expect("parsed manifest");
        let tc = parsed.target_config.expect("target_config");
        assert_eq!(
            tc.prebuilt.as_ref().expect("prebuilt"),
            &pkg_dir.join("./prebuilt/demo.lib")
        );
        let backend = tc.backends.first().expect("d3d12 backend");
        assert_eq!(backend.backend, NativeBackend::D3d12);
        assert_eq!(backend.libs, vec!["d3d12", "dxgi", "dxguid"]);
        assert_eq!(
            backend.shader_sources,
            vec![pkg_dir.join("shaders/default.hlsl")]
        );
    }

    #[test]
    fn rejects_d3d12_backend_on_linux_target() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pkg_dir = dir.path();
        let manifest = serde_json::json!({
            "perry": {
                "nativeLibrary": {
                    "functions": [],
                    "targets": {
                        "linux": {
                            "crate": "crate-linux",
                            "lib": "demo",
                            "backends": { "d3d12": { "libs": ["d3d12"] } }
                        }
                    }
                }
            }
        });
        std::fs::write(
            pkg_dir.join("package.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .expect("write package.json");

        let err = parse_native_library_manifest(pkg_dir, "demo", Some("linux"))
            .expect_err("d3d12 on linux must fail");
        let msg = err.to_string();
        assert!(msg.contains("d3d12"), "got: {msg}");
        assert!(msg.contains("Windows-only"), "got: {msg}");
    }

    #[test]
    fn rejects_unknown_backend_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pkg_dir = dir.path();
        let manifest = serde_json::json!({
            "perry": {
                "nativeLibrary": {
                    "functions": [],
                    "targets": {
                        "linux": {
                            "crate": "crate-linux",
                            "lib": "demo",
                            "backends": { "cuda": {} }
                        }
                    }
                }
            }
        });
        std::fs::write(
            pkg_dir.join("package.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .expect("write package.json");

        let err = parse_native_library_manifest(pkg_dir, "demo", Some("linux"))
            .expect_err("unknown backend must fail");
        let msg = err.to_string();
        assert!(msg.contains("cuda"), "got: {msg}");
        assert!(msg.contains("metal"), "got: {msg}");
        assert!(msg.contains("vulkan"), "got: {msg}");
        assert!(msg.contains("d3d12"), "got: {msg}");
    }

    #[test]
    fn parses_target_gated_backend_fixture() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pkg_dir = dir.path();
        std::fs::write(
            pkg_dir.join("package.json"),
            include_str!("../fixtures/native-target-packaging/target-gated-backends.package.json"),
        )
        .expect("write fixture package.json");
        std::fs::create_dir_all(pkg_dir.join("prebuilt/windows"))
            .expect("create prebuilt fixture dir");
        std::fs::write(pkg_dir.join("prebuilt/windows/demo.lib"), b"fixture")
            .expect("write prebuilt fixture lib");

        let linux = parse_native_library_manifest(pkg_dir, "@scope/demo-native", Some("linux"))
            .expect("parse linux manifest")
            .expect("linux manifest");
        let linux_tc = linux.target_config.expect("linux target_config");
        assert!(linux_tc.available);
        assert_eq!(linux_tc.lib_name, "demo_linux");
        assert_eq!(
            linux_tc.resources,
            vec![pkg_dir.join("assets/linux-config.json")]
        );
        let vulkan = linux_tc.backends.first().expect("vulkan backend");
        assert_eq!(vulkan.backend, NativeBackend::Vulkan);
        assert_eq!(vulkan.libs, vec!["vulkan"]);
        assert_eq!(vulkan.lib_dirs, vec![pkg_dir.join("vendor/vulkan/lib")]);
        assert_eq!(
            vulkan.shader_sources,
            vec![pkg_dir.join("shaders/default.comp")]
        );
        assert_eq!(
            vulkan.shader_outputs,
            vec![pkg_dir.join("prebuilt/default.spv")]
        );
        assert_eq!(vulkan.resources, vec![pkg_dir.join("resources/vulkan")]);
        assert_eq!(vulkan.package.name.as_deref(), Some("demo-vulkan"));
        assert_eq!(vulkan.package.kind.as_deref(), Some("spirv"));

        let android = parse_native_library_manifest(pkg_dir, "@scope/demo-native", Some("android"))
            .expect("parse android manifest")
            .expect("android manifest");
        let android_tc = android.target_config.expect("android target_config");
        assert!(!android_tc.available);
        assert_eq!(
            android_tc.unavailable_reason.as_deref(),
            Some("Android native package ships separately")
        );
        assert!(android_tc.backends.is_empty());

        let windows = parse_native_library_manifest(pkg_dir, "@scope/demo-native", Some("windows"))
            .expect("parse windows manifest")
            .expect("windows manifest");
        let windows_tc = windows.target_config.expect("windows target_config");
        assert_eq!(
            windows_tc.prebuilt.as_ref().expect("windows prebuilt"),
            &pkg_dir.join("./prebuilt/windows/demo.lib")
        );
        let d3d12 = windows_tc.backends.first().expect("d3d12 backend");
        assert_eq!(d3d12.backend, NativeBackend::D3d12);
        assert_eq!(d3d12.libs, vec!["d3d12", "dxgi", "dxguid"]);
        assert_eq!(
            d3d12.shader_sources,
            vec![pkg_dir.join("shaders/default.hlsl")]
        );
        assert_eq!(d3d12.package.kind.as_deref(), Some("dxil"));
    }

    #[test]
    fn unavailable_target_skips_without_build_metadata() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pkg_dir = dir.path();
        let manifest = serde_json::json!({
            "perry": {
                "nativeLibrary": {
                    "functions": [],
                    "targets": {
                        "linux": {
                            "available": false,
                            "unavailableReason": "GPU SDK not distributed for this target"
                        }
                    }
                }
            }
        });
        std::fs::write(
            pkg_dir.join("package.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .expect("write package.json");

        let parsed = parse_native_library_manifest(pkg_dir, "demo", Some("linux"))
            .expect("parse manifest")
            .expect("parsed manifest");
        let tc = parsed.target_config.expect("target_config");
        assert!(!tc.available);
        assert_eq!(
            tc.unavailable_reason.as_deref(),
            Some("GPU SDK not distributed for this target")
        );
        assert!(tc.backends.is_empty());
    }

    #[test]
    fn rejects_invalid_target_available_type() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pkg_dir = dir.path();
        let manifest = serde_json::json!({
            "perry": {
                "nativeLibrary": {
                    "functions": [],
                    "targets": {
                        "linux": {
                            "available": "false",
                            "crate": "crate-linux",
                            "lib": "demo"
                        }
                    }
                }
            }
        });
        std::fs::write(
            pkg_dir.join("package.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .expect("write package.json");

        let err = parse_native_library_manifest(pkg_dir, "demo", Some("linux"))
            .expect_err("string available must fail");
        let msg = err.to_string();
        assert!(msg.contains("targets.linux.available"), "got: {msg}");
        assert!(msg.contains("expected boolean"), "got: {msg}");
    }

    #[test]
    fn rejects_invalid_backend_available_type() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pkg_dir = dir.path();
        let manifest = serde_json::json!({
            "perry": {
                "nativeLibrary": {
                    "functions": [],
                    "targets": {
                        "linux": {
                            "crate": "crate-linux",
                            "lib": "demo",
                            "backends": {
                                "vulkan": {
                                    "available": "yes",
                                    "libs": ["vulkan"]
                                }
                            }
                        }
                    }
                }
            }
        });
        std::fs::write(
            pkg_dir.join("package.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .expect("write package.json");

        let err = parse_native_library_manifest(pkg_dir, "demo", Some("linux"))
            .expect_err("string backend available must fail");
        let msg = err.to_string();
        assert!(msg.contains("backends.vulkan.available"), "got: {msg}");
        assert!(msg.contains("expected boolean"), "got: {msg}");
    }
}

#[cfg(test)]
mod module_spec_tests {
    use super::split_module_spec;

    #[test]
    fn splits_scoped_package_and_subpath() {
        let (pkg, sub) = split_module_spec("@bloomengine/engine-darwin-arm64/lib/libbloom_macos.a")
            .expect("split");
        assert_eq!(pkg, "@bloomengine/engine-darwin-arm64");
        assert_eq!(sub, "lib/libbloom_macos.a");
    }

    #[test]
    fn splits_unscoped_package_and_subpath() {
        let (pkg, sub) = split_module_spec("esbuild-darwin-arm64/bin/esbuild").expect("split");
        assert_eq!(pkg, "esbuild-darwin-arm64");
        assert_eq!(sub, "bin/esbuild");
    }

    #[test]
    fn bare_scoped_package_without_subpath_rejected() {
        // `@scope/pkg` has no file to link — `prebuilt:` must name a
        // specific archive within the package.
        assert!(split_module_spec("@bloomengine/engine-darwin-arm64").is_none());
    }

    #[test]
    fn bare_unscoped_package_without_subpath_rejected() {
        assert!(split_module_spec("esbuild-darwin-arm64").is_none());
    }
}

#[cfg(test)]
mod declaration_sidecar_tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    fn write_typed_js_package(root: &Path, package_name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let package_dir = root.join("node_modules").join(package_name);
        std::fs::create_dir_all(package_dir.join("dist")).expect("mkdir package dist");
        let implementation = package_dir.join("dist/index.js");
        let declaration = package_dir.join("dist/index.d.ts");
        std::fs::write(&implementation, "export class Codex {}\n").expect("write js");
        std::fs::write(&declaration, "export declare class Codex {}\n").expect("write dts");
        std::fs::write(
            package_dir.join("package.json"),
            serde_json::json!({
                "name": package_name,
                "type": "module",
                "module": "./dist/index.js",
                "types": "./dist/index.d.ts",
                "exports": {
                    ".": {
                        "types": "./dist/index.d.ts",
                        "import": "./dist/index.js"
                    }
                }
            })
            .to_string(),
        )
        .expect("write package.json");
        (package_dir, implementation, declaration)
    }

    #[test]
    fn package_exports_types_resolve_as_declaration_sidecar() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (package_dir, implementation, declaration) =
            write_typed_js_package(dir.path(), "typed-js");

        let found = resolve_package_declaration_entry(&package_dir, None, Some(&implementation))
            .expect("declaration sidecar");

        assert_eq!(
            found,
            declaration.canonicalize().expect("canonical declaration")
        );
    }

    #[test]
    fn typed_node_modules_js_stays_interpreted_without_compile_package_opt_in() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let (_package_dir, implementation, declaration) = write_typed_js_package(root, "typed-js");
        let src_dir = root.join("src");
        std::fs::create_dir_all(&src_dir).expect("mkdir src");
        let importer = src_dir.join("main.ts");
        std::fs::write(&importer, "import { Codex } from 'typed-js';\n").expect("write importer");

        let resolved = resolve_import(
            "typed-js",
            &importer,
            root,
            &HashSet::new(),
            &HashMap::new(),
        )
        .expect("resolve typed-js");

        assert_eq!(resolved.1, ModuleKind::Interpreted);
        assert_eq!(
            resolved.0,
            implementation
                .canonicalize()
                .expect("canonical implementation")
        );
        assert_eq!(
            declaration_sidecar_for_resolved_import("typed-js", &resolved.0).expect("sidecar"),
            declaration.canonicalize().expect("canonical declaration")
        );
    }

    #[test]
    fn typed_node_modules_js_becomes_native_with_compile_package_opt_in() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let (_package_dir, implementation, declaration) = write_typed_js_package(root, "typed-js");
        let src_dir = root.join("src");
        std::fs::create_dir_all(&src_dir).expect("mkdir src");
        let importer = src_dir.join("main.ts");
        std::fs::write(&importer, "import { Codex } from 'typed-js';\n").expect("write importer");

        let compile_packages = HashSet::from(["typed-js".to_string()]);
        let resolved = resolve_import(
            "typed-js",
            &importer,
            root,
            &compile_packages,
            &HashMap::new(),
        )
        .expect("resolve typed-js");

        assert_eq!(resolved.1, ModuleKind::NativeCompiled);
        assert_eq!(
            resolved.0,
            implementation
                .canonicalize()
                .expect("canonical implementation")
        );
        assert_eq!(
            declaration_sidecar_for_resolved_import("typed-js", &resolved.0).expect("sidecar"),
            declaration.canonicalize().expect("canonical declaration")
        );
    }

    #[test]
    fn extract_compile_package_dir_uses_path_components() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let path = root
            .join("node_modules")
            .join("@noble")
            .join("curves")
            .join("node_modules")
            .join("@noble")
            .join("hashes")
            .join("src")
            .join("sha256.ts");

        assert_eq!(
            extract_compile_package_dir(&path, "@noble/hashes").expect("package dir"),
            root.join("node_modules")
                .join("@noble")
                .join("curves")
                .join("node_modules")
                .join("@noble")
                .join("hashes")
        );
        assert_eq!(
            extract_compile_package_dir(&path, "@noble/curves").expect("outer package dir"),
            root.join("node_modules").join("@noble").join("curves")
        );
    }

    #[test]
    fn compile_package_membership_uses_path_components() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let path = root
            .join("node_modules")
            .join("left-pad")
            .join("lib")
            .join("index.js");
        let compile_packages = HashSet::from(["left-pad".to_string()]);

        assert!(is_in_compile_package(&path, &compile_packages));
        assert!(!is_in_compile_package(
            &root
                .join("not_node_modules")
                .join("left-pad")
                .join("index.js"),
            &compile_packages
        ));
    }

    #[test]
    fn declaration_file_detection_includes_mts_and_cts_sidecars() {
        assert!(is_declaration_file(Path::new("index.d.ts")));
        assert!(is_declaration_file(Path::new("index.d.mts")));
        assert!(is_declaration_file(Path::new("index.d.cts")));
        assert!(!is_declaration_file(Path::new("index.ts")));
    }

    /// #3527 (blocker #4): `enumerate_installed_packages` must surface every
    /// package name in the `node_modules` tree — flat deps, `@scope/pkg`
    /// entries, and transitive deps nested under a package's own
    /// `node_modules` — so the `"*"` / `"@scope/*"` wildcard in
    /// `perry.compilePackages` can be expanded into concrete names. npm
    /// bookkeeping dirs (`.bin`, `.cache`) must be skipped.
    #[test]
    fn enumerate_installed_packages_covers_flat_scoped_and_nested() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let nm = root.join("node_modules");

        // Flat deps + npm bookkeeping that must be ignored.
        std::fs::create_dir_all(nm.join("express")).expect("mkdir express");
        std::fs::create_dir_all(nm.join("qs")).expect("mkdir qs");
        std::fs::create_dir_all(nm.join(".bin")).expect("mkdir .bin");
        std::fs::create_dir_all(nm.join(".cache")).expect("mkdir .cache");
        std::fs::write(nm.join(".package-lock.json"), b"{}").expect("write lockfile");

        // Scoped package.
        std::fs::create_dir_all(nm.join("@scope/pkg")).expect("mkdir @scope/pkg");

        // Transitive dep npm chose not to hoist (nested node_modules), plus a
        // nested scoped dep two levels down.
        std::fs::create_dir_all(nm.join("express/node_modules/nested-dep"))
            .expect("mkdir nested-dep");
        std::fs::create_dir_all(nm.join("express/node_modules/@deep/leaf"))
            .expect("mkdir @deep/leaf");

        let found = enumerate_installed_packages(root);

        assert!(found.contains("express"), "flat dep");
        assert!(found.contains("qs"), "flat dep");
        assert!(found.contains("@scope/pkg"), "scoped dep");
        assert!(found.contains("nested-dep"), "nested transitive dep");
        assert!(found.contains("@deep/leaf"), "nested scoped dep");
        assert!(!found.contains(".bin"), "npm bin dir skipped");
        assert!(!found.contains(".cache"), "npm cache dir skipped");
        assert!(
            !found.contains(".package-lock.json"),
            "lockfile (a file, not a dir) skipped"
        );
        assert_eq!(found.len(), 5, "exactly the five real packages");
    }
}
