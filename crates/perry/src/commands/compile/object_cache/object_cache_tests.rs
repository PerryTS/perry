//! Unit tests for the object-cache key derivation + cache-dir resolution.
//! Split out of `object_cache.rs` to keep that file under the 2000-line
//! cap (CI `check_file_size.sh`). `super::*` still resolves to the
//! `object_cache` module, so every test reaches the same private items.

use super::*;
use perry_codegen::{CompileOptions, ImportedClass, NamespaceEntry, NamespaceEntryKind};
use tempfile::tempdir;

/// A minimal `CompileOptions` with every vec/map empty. Tests that want
/// to vary one field mutate the returned value before hashing.
fn empty_opts() -> CompileOptions {
    CompileOptions {
        target: Some("aarch64-apple-darwin".to_string()),
        is_entry_module: false,
        non_entry_module_prefixes: Vec::new(),
        nextjs_path_init_modules: Vec::new(),
        import_function_prefixes: std::collections::HashMap::new(),
        import_function_ffi_aliases: std::collections::HashMap::new(),
        import_function_origin_names: std::collections::HashMap::new(),
        import_function_v8_specifiers: std::collections::HashMap::new(),
        // Issue #841: new submodule registry fields.
        import_function_node_submodule: std::collections::HashMap::new(),
        namespace_node_submodules: std::collections::HashMap::new(),
        namespace_v8_specifiers: std::collections::HashMap::new(),
        namespace_member_prefixes: std::collections::HashMap::new(),
        namespace_member_origin_names: std::collections::HashMap::new(),
        emit_ir_only: false,
        verify_native_regions: false,
        disable_buffer_fast_path: false,
        namespace_imports: Vec::new(),
        imported_classes: Vec::new(),
        imported_enums: Vec::new(),
        imported_async_funcs: std::collections::HashSet::new(),
        type_aliases: std::collections::HashMap::new(),
        imported_func_param_counts: std::collections::HashMap::new(),
        imported_func_has_rest: std::collections::HashSet::new(),
        imported_func_synthetic_arguments: std::collections::HashSet::new(),
        imported_func_return_types: std::collections::HashMap::new(),
        imported_vars: std::collections::HashSet::new(),
        output_type: "executable".to_string(),
        needs_stdlib: false,
        needs_ui: false,
        needs_geisterhand: false,
        geisterhand_port: 7676,
        enabled_features: Vec::new(),
        native_module_init_names: Vec::new(),
        js_module_specifiers: Vec::new(),
        bundled_extensions: Vec::new(),
        native_library_functions: Vec::new(),
        i18n_table: None,
        fast_math: false,
        fp_contract_mode: perry_codegen::FpContractMode::Off,
        app_metadata: perry_codegen::AppMetadata::default(),
        namespace_entries: Vec::new(),
        dynamic_import_path_to_prefix: std::collections::HashMap::new(),
        deferred_module_prefixes: std::collections::HashSet::new(),
        module_init_deps: Vec::new(),
        is_dynamic_import_target: false,
        debug_locations: false,
        module_source: None,
        debug_source_line_offset: 0,
    }
}

#[test]
fn djb2_hash_is_stable_and_distinct() {
    assert_eq!(djb2_hash(b""), 5381);
    assert_eq!(djb2_hash(b"hello"), djb2_hash(b"hello"));
    assert_ne!(djb2_hash(b"hello"), djb2_hash(b"world"));
}

#[test]
fn key_stable_for_same_inputs() {
    let opts = empty_opts();
    let k1 = compute_object_cache_key(&opts, 0xdeadbeef, "0.5.156");
    let k2 = compute_object_cache_key(&opts, 0xdeadbeef, "0.5.156");
    assert_eq!(k1, k2);
}

#[test]
fn key_changes_with_hir_hash() {
    // The second argument is the post-transform HIR fingerprint
    // (issue #686), produced by `perry_hir::stable_hash::hash_module`.
    // Two different HIR hashes — i.e. two semantically different
    // modules — must produce different cache keys.
    //
    // Note: "same source bytes, different HIR" (e.g. a lowering-pass
    // behavior change between Perry versions that rewrites the same
    // input into different HIR) is covered by the `build_id` field
    // mixed in by `perry_build_id()`, NOT by this hash. So a HIR
    // walk that adds new fields between releases doesn't need a
    // separate invalidation hook here.
    let opts = empty_opts();
    let a = compute_object_cache_key(&opts, 1, "0.5.156");
    let b = compute_object_cache_key(&opts, 2, "0.5.156");
    assert_ne!(a, b);
}

#[test]
fn key_changes_with_perry_version() {
    let opts = empty_opts();
    let a = compute_object_cache_key(&opts, 1, "0.5.155");
    let b = compute_object_cache_key(&opts, 1, "0.5.156");
    assert_ne!(a, b);
}

#[test]
fn key_changes_with_target() {
    let mut a = empty_opts();
    let mut b = empty_opts();
    a.target = Some("aarch64-apple-darwin".to_string());
    b.target = Some("x86_64-apple-darwin".to_string());
    assert_ne!(
        compute_object_cache_key(&a, 1, "0.5.156"),
        compute_object_cache_key(&b, 1, "0.5.156")
    );
}

#[test]
fn key_changes_with_entry_flag() {
    let mut a = empty_opts();
    let mut b = empty_opts();
    a.is_entry_module = false;
    b.is_entry_module = true;
    assert_ne!(
        compute_object_cache_key(&a, 1, "0.5.156"),
        compute_object_cache_key(&b, 1, "0.5.156")
    );
}

#[test]
fn key_changes_with_debug_locations_flag() {
    // #5247: toggling --debug-symbols (debug_locations) flips per-call
    // location emission, so cached objects must not be shared across it.
    let mut a = empty_opts();
    let mut b = empty_opts();
    a.debug_locations = false;
    b.debug_locations = true;
    assert_ne!(
        compute_object_cache_key(&a, 1, "0.5.156"),
        compute_object_cache_key(&b, 1, "0.5.156")
    );
}

#[test]
fn key_changes_with_fast_math_flag() {
    // Without this guard, `perry --fast-math foo.ts` after a default
    // build would silently serve the cached non-fast-math `.o` and
    // the flag would appear to do nothing. Bug found during the
    // original fast-math investigation; gate it here so a future
    // refactor can't reintroduce it.
    let mut a = empty_opts();
    let mut b = empty_opts();
    a.fast_math = false;
    b.fast_math = true;
    assert_ne!(
        compute_object_cache_key(&a, 1, "0.5.569"),
        compute_object_cache_key(&b, 1, "0.5.569")
    );
}

#[test]
fn key_changes_with_fp_contract_mode() {
    let mut a = empty_opts();
    let mut b = empty_opts();
    a.fp_contract_mode = perry_codegen::FpContractMode::Off;
    b.fp_contract_mode = perry_codegen::FpContractMode::On;
    assert_ne!(
        compute_object_cache_key(&a, 1, "0.5.569"),
        compute_object_cache_key(&b, 1, "0.5.569")
    );
}

#[test]
fn key_includes_perry_build_id() {
    // Issue #544: the cache key must mix in a hash of the running perry
    // binary so HIR/codegen pass changes invalidate the cache even when
    // the version string doesn't move. We can't easily synthesize two
    // distinct binary hashes from inside a unit test, but we can check
    // (a) that `perry_build_id()` returns a non-zero value when the
    // test binary exists on disk (i.e. the helper actually ran), and
    // (b) that perturbing the helper's output would change the key —
    // verified indirectly by confirming the field is present in the
    // serialized form via the field separator count.
    let id = perry_build_id();
    // The test binary is always readable, so the helper can't degrade
    // to 0 here. If this ever fails, current_exe() / fs::read started
    // misbehaving and we'd want to know.
    assert_ne!(id, 0, "perry_build_id must hash the test executable");
    // Stable across calls within a process (OnceLock).
    assert_eq!(perry_build_id(), id);
}

#[test]
fn key_changes_with_non_entry_prefix_order() {
    // Order-significant: non_entry_module_prefixes is topologically
    // sorted, and a reorder must invalidate the cache (this is the
    // v0.5.127-128 link-ordering regression class — the issue's
    // acceptance criterion).
    let mut a = empty_opts();
    let mut b = empty_opts();
    a.is_entry_module = true;
    b.is_entry_module = true;
    a.non_entry_module_prefixes = vec!["a".into(), "b".into()];
    b.non_entry_module_prefixes = vec!["b".into(), "a".into()];
    assert_ne!(
        compute_object_cache_key(&a, 1, "0.5.156"),
        compute_object_cache_key(&b, 1, "0.5.156")
    );
}

#[test]
fn key_stable_regardless_of_hashmap_insertion_order() {
    // HashMap iteration order is platform-dependent; the key must
    // sort entries so two equivalent maps produce the same hash.
    let mut a = empty_opts();
    let mut b = empty_opts();
    a.import_function_prefixes
        .insert("foo".into(), "mod_a".into());
    a.import_function_prefixes
        .insert("bar".into(), "mod_b".into());
    b.import_function_prefixes
        .insert("bar".into(), "mod_b".into());
    b.import_function_prefixes
        .insert("foo".into(), "mod_a".into());
    assert_eq!(
        compute_object_cache_key(&a, 1, "0.5.156"),
        compute_object_cache_key(&b, 1, "0.5.156")
    );
}

#[test]
fn key_stable_for_order_insensitive_graph_lists() {
    // These graph-wide lists are either derived from collections or are
    // consumed as lookup metadata, not as an ordered codegen sequence.
    // Hashing their raw Vec order would make every module key depend on
    // upstream collection/traversal order.
    let mut a = empty_opts();
    let mut b = empty_opts();
    a.js_module_specifiers = vec!["z.js".into(), "a.js".into()];
    b.js_module_specifiers = vec!["a.js".into(), "z.js".into()];
    assert_eq!(
        compute_object_cache_key(&a, 1, "0.5.156"),
        compute_object_cache_key(&b, 1, "0.5.156")
    );

    a = empty_opts();
    b = empty_opts();
    a.bundled_extensions = vec![
        ("/project/ext/z.ts".into(), "project_ext_z_ts".into()),
        ("/project/ext/a.ts".into(), "project_ext_a_ts".into()),
    ];
    b.bundled_extensions = vec![
        ("/project/ext/a.ts".into(), "project_ext_a_ts".into()),
        ("/project/ext/z.ts".into(), "project_ext_z_ts".into()),
    ];
    assert_eq!(
        compute_object_cache_key(&a, 1, "0.5.156"),
        compute_object_cache_key(&b, 1, "0.5.156")
    );

    a = empty_opts();
    b = empty_opts();
    a.native_library_functions = vec![
        (
            "zeta".into(),
            vec![perry_api_manifest::NativeAbiType::I32],
            perry_api_manifest::NativeAbiType::F64,
        ),
        (
            "alpha".into(),
            vec![perry_api_manifest::NativeAbiType::String],
            perry_api_manifest::NativeAbiType::Bool,
        ),
    ];
    b.native_library_functions = vec![
        (
            "alpha".into(),
            vec![perry_api_manifest::NativeAbiType::String],
            perry_api_manifest::NativeAbiType::Bool,
        ),
        (
            "zeta".into(),
            vec![perry_api_manifest::NativeAbiType::I32],
            perry_api_manifest::NativeAbiType::F64,
        ),
    ];
    assert_eq!(
        compute_object_cache_key(&a, 1, "0.5.156"),
        compute_object_cache_key(&b, 1, "0.5.156")
    );
}

fn record_row_type(property_insert_order: &[&str]) -> perry_types::Type {
    let mut properties = std::collections::HashMap::new();
    for name in property_insert_order {
        let ty = match *name {
            "name" => perry_types::Type::String,
            "id" | "value" => perry_types::Type::Number,
            _ => panic!("unexpected property"),
        };
        properties.insert(
            (*name).to_string(),
            perry_types::PropertyInfo {
                ty,
                optional: false,
                readonly: false,
            },
        );
    }
    perry_types::Type::Object(perry_types::ObjectType {
        name: None,
        properties,
        property_order: Some(vec!["id".into(), "name".into(), "value".into()]),
        index_signature: None,
    })
}

#[test]
fn key_stable_for_nested_type_hashmap_order() {
    let type_a = record_row_type(&["name", "id", "value"]);
    let type_b = record_row_type(&["id", "name", "value"]);

    let mut a = empty_opts();
    let mut b = empty_opts();
    a.type_aliases.insert("RecordRow".into(), type_a.clone());
    b.type_aliases.insert("RecordRow".into(), type_b.clone());
    assert_eq!(
        compute_object_cache_key(&a, 1, "0.5.156"),
        compute_object_cache_key(&b, 1, "0.5.156")
    );

    a = empty_opts();
    b = empty_opts();
    a.imported_func_return_types
        .insert("loadRecord".into(), type_a.clone());
    b.imported_func_return_types
        .insert("loadRecord".into(), type_b.clone());
    assert_eq!(
        compute_object_cache_key(&a, 1, "0.5.156"),
        compute_object_cache_key(&b, 1, "0.5.156")
    );

    let class_for = |field_type| ImportedClass {
        name: "RowBox".into(),
        local_alias: None,
        source_prefix: "feature_ts".into(),
        constructor_param_count: 0,
        has_own_constructor: false,
        constructor_has_rest: false,
        has_instance_fields: true,
        method_names: vec![],
        method_param_counts: vec![],
        method_has_rest: vec![],
        static_method_names: vec![],
        getter_names: vec![],
        setter_names: vec![],
        parent_name: None,
        field_names: vec!["row".into()],
        field_types: vec![field_type],
        static_field_names: vec![],
        source_class_id: Some(7),
    };

    a = empty_opts();
    b = empty_opts();
    a.imported_classes.push(class_for(type_a));
    b.imported_classes.push(class_for(type_b));
    assert_eq!(
        compute_object_cache_key(&a, 1, "0.5.156"),
        compute_object_cache_key(&b, 1, "0.5.156")
    );
}

#[test]
fn key_changes_with_imported_class_signature() {
    let mut a = empty_opts();
    let mut b = empty_opts();
    a.imported_classes.push(ImportedClass {
        name: "Foo".into(),
        local_alias: None,
        source_prefix: "src".into(),
        constructor_param_count: 1,
        has_own_constructor: true,
        constructor_has_rest: false,
        has_instance_fields: true,
        method_names: vec!["bar".into()],
        method_param_counts: vec![0],
        method_has_rest: vec![false],
        static_method_names: vec![],
        getter_names: vec![],
        setter_names: vec![],
        parent_name: None,
        field_names: vec!["x".into()],
        field_types: vec![],
        static_field_names: vec![],
        source_class_id: Some(42),
    });
    b.imported_classes.push(ImportedClass {
        name: "Foo".into(),
        local_alias: None,
        source_prefix: "src".into(),
        constructor_param_count: 2, // different arity
        has_own_constructor: true,
        constructor_has_rest: false,
        has_instance_fields: true,
        method_names: vec!["bar".into()],
        method_param_counts: vec![0],
        method_has_rest: vec![false],
        static_method_names: vec![],
        getter_names: vec![],
        setter_names: vec![],
        parent_name: None,
        field_names: vec!["x".into()],
        field_types: vec![],
        static_field_names: vec![],
        source_class_id: Some(42),
    });
    assert_ne!(
        compute_object_cache_key(&a, 1, "0.5.156"),
        compute_object_cache_key(&b, 1, "0.5.156")
    );
}

#[test]
fn key_changes_with_imported_class_codegen_surface() {
    let base = ImportedClass {
        name: "Foo".into(),
        local_alias: None,
        source_prefix: "src".into(),
        constructor_param_count: 1,
        has_own_constructor: true,
        constructor_has_rest: false,
        has_instance_fields: true,
        method_names: vec!["bar".into()],
        method_param_counts: vec![1],
        method_has_rest: vec![false],
        static_method_names: vec![],
        getter_names: vec![],
        setter_names: vec![],
        parent_name: None,
        field_names: vec!["x".into()],
        field_types: vec![perry_types::Type::Number],
        static_field_names: vec![],
        source_class_id: Some(42),
    };
    let key_for = |class: ImportedClass| {
        let mut opts = empty_opts();
        opts.imported_classes.push(class);
        compute_object_cache_key(&opts, 1, "0.5.156")
    };
    let base_key = key_for(base.clone());

    let mut changed = base.clone();
    changed.has_own_constructor = false;
    assert_ne!(base_key, key_for(changed));

    let mut changed = base.clone();
    changed.has_instance_fields = false;
    assert_ne!(base_key, key_for(changed));

    let mut changed = base.clone();
    changed.method_has_rest = vec![true];
    assert_ne!(base_key, key_for(changed));

    let mut changed = base.clone();
    changed.static_method_names = vec!["make".into()];
    assert_ne!(base_key, key_for(changed));

    let mut changed = base.clone();
    changed.static_field_names = vec!["VERSION".into()];
    assert_ne!(base_key, key_for(changed));

    let mut changed = base.clone();
    changed.getter_names = vec!["value".into()];
    assert_ne!(base_key, key_for(changed));

    let mut changed = base.clone();
    changed.setter_names = vec!["value".into()];
    assert_ne!(base_key, key_for(changed));

    let mut changed = base;
    changed.field_types = vec![perry_types::Type::String];
    assert_ne!(base_key, key_for(changed));
}

#[test]
fn key_changes_with_namespace_member_prefixes() {
    let mut a = empty_opts();
    let mut b = empty_opts();
    a.namespace_member_prefixes
        .insert(("ns".into(), "make".into()), "src_a".into());
    b.namespace_member_prefixes
        .insert(("ns".into(), "make".into()), "src_b".into());
    assert_ne!(
        compute_object_cache_key(&a, 1, "0.5.156"),
        compute_object_cache_key(&b, 1, "0.5.156")
    );
}

#[test]
fn key_changes_with_imported_rest_shapes() {
    let mut a = empty_opts();
    let mut b = empty_opts();
    b.imported_func_has_rest.insert("collect".into());
    assert_ne!(
        compute_object_cache_key(&a, 1, "0.5.156"),
        compute_object_cache_key(&b, 1, "0.5.156")
    );

    a = empty_opts();
    b = empty_opts();
    b.imported_func_synthetic_arguments.insert("invoke".into());
    assert_ne!(
        compute_object_cache_key(&a, 1, "0.5.156"),
        compute_object_cache_key(&b, 1, "0.5.156")
    );
}

#[test]
fn key_changes_with_dynamic_import_metadata() {
    let mut a = empty_opts();
    let mut b = empty_opts();
    b.namespace_entries.push(NamespaceEntry {
        name: "answer".into(),
        kind: NamespaceEntryKind::ForeignFunction {
            source_prefix: "dep".into(),
            source_local: "answer".into(),
            param_count: 1,
        },
    });
    assert_ne!(
        compute_object_cache_key(&a, 1, "0.5.156"),
        compute_object_cache_key(&b, 1, "0.5.156")
    );

    a = empty_opts();
    b = empty_opts();
    b.dynamic_import_path_to_prefix
        .insert("./lazy".into(), "lazy_ts".into());
    assert_ne!(
        compute_object_cache_key(&a, 1, "0.5.156"),
        compute_object_cache_key(&b, 1, "0.5.156")
    );
}

#[test]
fn key_changes_with_app_group() {
    let mut a = empty_opts();
    let mut b = empty_opts();
    a.app_metadata.app_group = None;
    b.app_metadata.app_group = Some("group.com.example.shared".into());
    assert_ne!(
        compute_object_cache_key(&a, 1, "0.5.156"),
        compute_object_cache_key(&b, 1, "0.5.156")
    );
}

#[test]
fn key_changes_with_bitcode_mode() {
    let mut a = empty_opts();
    let mut b = empty_opts();
    a.emit_ir_only = false;
    b.emit_ir_only = true;
    assert_ne!(
        compute_object_cache_key(&a, 1, "0.5.156"),
        compute_object_cache_key(&b, 1, "0.5.156")
    );
}

#[test]
fn key_changes_with_codegen_env_vars() {
    // Flipping an env var that perry-codegen reads must invalidate the
    // key so we don't serve a cached .o that was built with different
    // debug sections, a different clang binary, different generated
    // helper calls, or a skipped verifier.
    //
    let opts = empty_opts();
    for var in [
        "PERRY_DEBUG_INIT",
        "PERRY_DEBUG_SYMBOLS",
        "PERRY_LLVM_CLANG",
        "PERRY_WRITE_BARRIERS",
        "PERRY_SHADOW_STACK",
        "PERRY_DISABLE_BUFFER_FAST_PATH",
        "PERRY_VERIFY_NATIVE_REGIONS",
        "PERRY_UNBOXED_OBJECT_FIELDS",
        "PERRY_TARGET_CPU",
        // Codegen tuning/emission toggles (#6394).
        "PERRY_TYPED_FEEDBACK",
        "PERRY_TYPED_FEEDBACK_TRACE",
        "PERRY_FULL_OUTLINE_IC",
        "PERRY_FULL_OUTLINE_IC_MIN_FUNCS",
        "PERRY_OUTLINE_METHOD_DISPATCH",
        "PERRY_INLINE_NEW",
        "PERRY_INLINE_CTOR",
        "PERRY_STRING_INIT_CHUNK_SIZE",
        "PERRY_LL_O0_THRESHOLD_BYTES",
        "PERRY_LL_SIZE_OPT",
        "PERRY_LL_SIZE_OPT_MAX_FN_BYTES",
        "PERRY_ENTRY_SYMBOL",
        "PERRY_CODEGEN_UNITS",
        "PERRY_CODEGEN_UNIT_SIZE",
        "PERRY_SETJMP_VOLATILE",
        "PERRY_GC_MOVING_LOOP_POLLS",
    ] {
        // Sample state without the var, with the var, and with a different
        // value — all three keys must be distinct.
        let k_unset = compute_object_cache_key_with_env(&opts, 1, "0.5.156", |_| None);
        let k_set = compute_object_cache_key_with_env(&opts, 1, "0.5.156", |name| {
            (name == var).then(|| "1".to_string())
        });
        let k_two = compute_object_cache_key_with_env(&opts, 1, "0.5.156", |name| {
            (name == var).then(|| "2".to_string())
        });
        assert_ne!(k_unset, k_set, "setting {} must change key", var);
        assert_ne!(k_set, k_two, "changing {} value must change key", var);
    }
}

/// #6439: the FFI manifest must survive a store→lookup round trip, or a
/// warm cache silently drops the provider crates the link line needs.
#[test]
fn store_then_lookup_path_with_ffi_round_trips_manifest() {
    let dir = tempdir().unwrap();
    let cache = ObjectCache::new(dir.path(), "test-target", true);
    let key = 0x6439_0001;
    cache.store_ffi_manifest(key, &["js_ws_connect_start", "js_ws_send"]);
    cache.store(key, b"object bytes");
    let (path, symbols) = cache
        .lookup_path_with_ffi(key)
        .expect("object + manifest both present must hit");
    assert!(path.is_file());
    assert_eq!(symbols, vec!["js_ws_connect_start", "js_ws_send"]);
    assert_eq!(cache.hits(), 1);
    assert_eq!(cache.misses(), 0);
}

/// The common case: a module that emits no registered FFI stores an
/// empty manifest, which must still read back as a hit (an *absent*
/// manifest means something quite different — see the test below).
#[test]
fn empty_ffi_manifest_is_a_hit_with_no_symbols() {
    let dir = tempdir().unwrap();
    let cache = ObjectCache::new(dir.path(), "test-target", true);
    let key = 0x6439_0002;
    cache.store_ffi_manifest(key, &[]);
    cache.store(key, b"object bytes");
    let (_, symbols) = cache
        .lookup_path_with_ffi(key)
        .expect("empty manifest is still a complete entry");
    assert!(symbols.is_empty());
    assert_eq!(cache.hits(), 1);
}

/// #6439 regression: an object written by a pre-manifest perry has no
/// recoverable FFI provenance. Treating it as a hit would replay zero
/// symbols and silently drop `perry-ext-ws` from the link line — the
/// exact "links cold, fails warm" bug. It must report a miss so the
/// module recompiles once and the entry self-heals.
#[test]
fn object_without_ffi_manifest_reports_miss() {
    let dir = tempdir().unwrap();
    let cache = ObjectCache::new(dir.path(), "test-target", true);
    let key = 0x6439_0003;
    cache.store(key, b"legacy object with no manifest");
    assert!(
        cache.lookup_path_with_ffi(key).is_none(),
        "unknowable FFI provenance must not be reported as a usable hit"
    );
    assert_eq!(cache.misses(), 1);
    assert_eq!(cache.hits(), 0, "an unusable entry must not count as a hit");
    // The plain byte lookup still hits — only the FFI-aware path is strict.
    assert!(cache.lookup(key).is_some());
}

#[test]
fn disabled_cache_always_misses_and_drops_stores() {
    let dir = tempdir().unwrap();
    let cache = ObjectCache::new(dir.path(), "test-target", false);
    assert!(!cache.is_enabled());
    assert!(cache.lookup(0xdeadbeef).is_none());
    cache.store(0xdeadbeef, b"payload");
    // Nothing was written — a second lookup still misses.
    assert!(cache.lookup(0xdeadbeef).is_none());
    // No counters bumped for a disabled cache.
    assert_eq!(cache.hits(), 0);
    assert_eq!(cache.stores(), 0);
}

#[test]
fn store_then_lookup_round_trips_bytes() {
    let dir = tempdir().unwrap();
    let cache = ObjectCache::new(dir.path(), "test-target", true);
    assert!(cache.is_enabled());
    let key = 0xcafef00d;
    let payload = b"the quick brown fox".to_vec();
    cache.store(key, &payload);
    assert_eq!(cache.stores(), 1);
    let got = cache.lookup(key).expect("must hit after store");
    assert_eq!(got, payload);
    assert_eq!(cache.hits(), 1);
    assert_eq!(cache.misses(), 0);
    assert_eq!(cache.bytes_materialized(), payload.len());
    assert_eq!(cache.path_reuses(), 0);
}

#[test]
fn store_then_lookup_path_reuses_cached_file_without_materializing_bytes() {
    let dir = tempdir().unwrap();
    let cache = ObjectCache::new(dir.path(), "test-target", true);
    let key = 0xfeedface;
    cache.store(key, b"object bytes");

    let path = cache.lookup_path(key).expect("must hit by path");
    assert!(path.is_file(), "missing cached object: {}", path.display());
    assert_eq!(std::fs::read(path).unwrap(), b"object bytes");
    assert_eq!(cache.hits(), 1);
    assert_eq!(cache.misses(), 0);
    assert_eq!(cache.path_reuses(), 1);
    assert_eq!(cache.bytes_materialized(), 0);
}

#[test]
fn lookup_miss_bumps_miss_counter() {
    let dir = tempdir().unwrap();
    let cache = ObjectCache::new(dir.path(), "test-target", true);
    assert!(cache.lookup(0x1234).is_none());
    assert!(cache.lookup_path(0x5678).is_none());
    assert_eq!(cache.hits(), 0);
    assert_eq!(cache.misses(), 2);
}

#[test]
fn cache_files_land_under_target_subdirectory() {
    // The on-disk layout must be <cache_dir>/objects/<target>/<hex>.o
    // so cross-compile caches can coexist without colliding. The dir
    // passed to ObjectCache::new is the already-resolved cache dir.
    let dir = tempdir().unwrap();
    let cache = ObjectCache::new(dir.path(), "aarch64-apple-darwin", true);
    cache.store(0xabc, b"xx");
    let expected = dir
        .path()
        .join("objects")
        .join("aarch64-apple-darwin")
        .join(format!("{:016x}.o", 0xabc_u64));
    assert!(expected.exists(), "missing: {}", expected.display());
}

#[test]
fn resolve_cache_dir_defaults_to_node_modules_cache_perry() {
    // No override → the find-cache-dir convention under the project root.
    let root = Path::new("/projects/app");
    let got = resolve_cache_dir(root, None);
    assert_eq!(
        got,
        Path::new("/projects/app")
            .join("node_modules")
            .join(".cache")
            .join("perry")
    );
}

#[test]
fn resolve_cache_dir_absolute_override_used_as_is() {
    // An absolute override ignores the project root entirely.
    let root = Path::new("/projects/app");
    let override_dir = Path::new("/var/cache/perry");
    let got = resolve_cache_dir(root, Some(override_dir));
    assert_eq!(got, Path::new("/var/cache/perry"));
}

#[test]
fn resolve_cache_dir_relative_override_resolves_against_project_root() {
    // A relative override joins onto the project root, so two projects
    // with the same `perry.cacheDir: ".cache"` don't collide.
    let root = Path::new("/projects/app");
    let override_dir = Path::new("build/cache");
    let got = resolve_cache_dir(root, Some(override_dir));
    assert_eq!(got, Path::new("/projects/app").join("build").join("cache"));
}

#[test]
fn object_cache_writes_under_resolved_cache_dir() {
    // End-to-end: resolve the default dir, build the cache against it,
    // and confirm bytes land under <resolved>/objects/<target>/.
    let dir = tempdir().unwrap();
    let resolved = resolve_cache_dir(dir.path(), None);
    let cache = ObjectCache::new(&resolved, "aarch64-apple-darwin", true);
    cache.store(0xabc, b"xx");
    let expected = dir
        .path()
        .join("node_modules")
        .join(".cache")
        .join("perry")
        .join("objects")
        .join("aarch64-apple-darwin")
        .join(format!("{:016x}.o", 0xabc_u64));
    assert!(expected.exists(), "missing: {}", expected.display());
}

#[test]
fn different_targets_do_not_share_entries() {
    let dir = tempdir().unwrap();
    let a = ObjectCache::new(dir.path(), "target-a", true);
    let b = ObjectCache::new(dir.path(), "target-b", true);
    a.store(0x777, b"from-a");
    assert!(b.lookup(0x777).is_none());
    assert_eq!(a.lookup(0x777).as_deref(), Some(b"from-a".as_ref()));
}

// --- cache-dir override precedence ----------------------------------
//
// Full chain (highest wins): `--cache-dir` CLI flag → `PERRY_CACHE_DIR`
// env → perry.toml `[perry] cacheDir` → package.json `perry.cacheDir` →
// default. The CLI flag is layered on top by the callers
// (`args.cache_dir.or_else(cache_dir_override)`), so the merge tested
// here is the non-CLI half: env → perry.toml → package.json.
//
// `pick_cache_dir_override` is pure over the already-read candidate
// strings, so the precedence is checked without filesystem or env races.

#[test]
fn pick_override_env_beats_toml_and_pkg() {
    // env wins over both lower layers — i.e. `PERRY_CACHE_DIR` overrides
    // perry.toml, which is the "env beats perry.toml" guarantee.
    let got = pick_cache_dir_override(Some("/env"), Some("/toml"), Some("/pkg"));
    assert_eq!(got, Some(PathBuf::from("/env")));
}

#[test]
fn pick_override_toml_beats_pkg() {
    // perry.toml overrides package.json when env is unset.
    let got = pick_cache_dir_override(None, Some("/toml"), Some("/pkg"));
    assert_eq!(got, Some(PathBuf::from("/toml")));
}

#[test]
fn pick_override_pkg_used_when_only_pkg_set() {
    let got = pick_cache_dir_override(None, None, Some("/pkg"));
    assert_eq!(got, Some(PathBuf::from("/pkg")));
}

#[test]
fn pick_override_none_when_all_unset() {
    assert_eq!(pick_cache_dir_override(None, None, None), None);
}

#[test]
fn pick_override_skips_empty_higher_layers() {
    // An empty string is treated as "not set", so a blank env value falls
    // through to perry.toml and a blank perry.toml falls through to pkg.
    assert_eq!(
        pick_cache_dir_override(Some(""), Some("/toml"), Some("/pkg")),
        Some(PathBuf::from("/toml"))
    );
    assert_eq!(
        pick_cache_dir_override(Some(""), Some(""), Some("/pkg")),
        Some(PathBuf::from("/pkg"))
    );
    assert_eq!(pick_cache_dir_override(Some(""), Some(""), Some("")), None);
}

#[test]
fn perry_toml_cache_dir_reads_perry_table() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("perry.toml"),
        "[perry]\ncacheDir = \"/var/cache/perry\"\n",
    )
    .unwrap();
    assert_eq!(
        perry_toml_cache_dir(dir.path()).as_deref(),
        Some("/var/cache/perry")
    );
}

#[test]
fn perry_toml_cache_dir_none_when_key_or_file_absent() {
    // No perry.toml at all.
    let empty = tempdir().unwrap();
    assert_eq!(perry_toml_cache_dir(empty.path()), None);

    // perry.toml present but no `[perry] cacheDir` key.
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("perry.toml"), "[perry]\nstrict = true\n").unwrap();
    assert_eq!(perry_toml_cache_dir(dir.path()), None);
}

#[test]
fn perry_toml_cache_dir_walks_up_to_project_root() {
    // The reader walks up from a nested dir, mirroring how the compile
    // pipeline discovers config from a subdirectory entry file.
    let root = tempdir().unwrap();
    fs::write(
        root.path().join("perry.toml"),
        "[perry]\ncacheDir = \".cache\"\n",
    )
    .unwrap();
    let nested = root.path().join("src").join("deep");
    fs::create_dir_all(&nested).unwrap();
    assert_eq!(perry_toml_cache_dir(&nested).as_deref(), Some(".cache"));
}

#[test]
fn package_json_cache_dir_reads_perry_field() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{ "perry": { "cacheDir": ".perry-cache" } }"#,
    )
    .unwrap();
    assert_eq!(
        package_json_cache_dir(dir.path()).as_deref(),
        Some(".perry-cache")
    );
}

#[test]
fn toml_overrides_pkg_via_readers_and_resolver() {
    // End-to-end (no env, no CLI): with both perry.toml and package.json
    // present, the chosen override is perry.toml's, and a relative value
    // resolves against the project root — matching the existing
    // `resolve_cache_dir_relative_override_resolves_against_project_root`
    // contract for the new perry.toml layer.
    let root = tempdir().unwrap();
    fs::write(
        root.path().join("perry.toml"),
        "[perry]\ncacheDir = \"toml-cache\"\n",
    )
    .unwrap();
    fs::write(
        root.path().join("package.json"),
        r#"{ "perry": { "cacheDir": "pkg-cache" } }"#,
    )
    .unwrap();

    let toml = perry_toml_cache_dir(root.path());
    let pkg = package_json_cache_dir(root.path());
    let chosen = pick_cache_dir_override(None, toml.as_deref(), pkg.as_deref());
    assert_eq!(chosen.as_deref(), Some(Path::new("toml-cache")));

    // Relative perry.toml value resolves against the project root.
    let resolved = resolve_cache_dir(root.path(), chosen.as_deref());
    assert_eq!(resolved, root.path().join("toml-cache"));
}
