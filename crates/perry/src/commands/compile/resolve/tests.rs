use super::*;

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
        assert_eq!(function.returns.to_string(), "promise<f64>");
        assert!(matches!(parsed.functions[1].returns, NativeAbiType::Void));
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
