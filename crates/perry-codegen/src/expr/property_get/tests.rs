//! Cargo-test-visible property-get codegen regressions.
//!
//! #5247's integration twin (`crates/perry/tests/
//! issue_5247_property_read_source_location.rs`) compiles + runs a real program
//! and only executes on nightly/tag workflows; the tests here assert codegen
//! contracts directly on emitted LLVM IR so they run on every PR (#5960
//! guideline).
//!
//! Contract: a general `Expr::PropertyGet` carrying a non-zero `byte_offset`
//! emits a `js_set_call_location` call in `lower_generic_property_get` under a
//! debug-location context (`--debug-symbols`), and emits NONE without it (the
//! default build stays overhead-free / byte-identical).

use crate::{compile_module, AppMetadata, CompileOptions};
use perry_hir::{Expr, Module, ModuleInitKind, Stmt};

#[path = "named_accessor_tests.rs"]
mod named_accessor;

fn ir_opts(debug_locations: bool, module_source: Option<&str>) -> CompileOptions {
    CompileOptions {
        static_shape_ids: Vec::new(),
        program_class_shape_ids: Default::default(),
        program_facts: Default::default(),
        program_layouts: Default::default(),
        gc_effects: Default::default(),
        target: None,
        is_entry_module: true,
        non_entry_module_prefixes: Vec::new(),
        thread_literal_module_prefixes: Vec::new(),
        nextjs_path_init_modules: Vec::new(),
        import_function_prefixes: std::collections::HashMap::new(),
        import_function_ffi_aliases: std::collections::HashMap::new(),
        import_function_origin_names: std::collections::HashMap::new(),
        import_function_v8_specifiers: std::collections::HashMap::new(),
        import_function_node_submodule: std::collections::HashMap::new(),
        namespace_node_submodules: std::collections::HashMap::new(),
        namespace_v8_specifiers: std::collections::HashMap::new(),
        namespace_member_prefixes: std::collections::HashMap::new(),
        namespace_member_origin_names: std::collections::HashMap::new(),
        emit_ir_only: true,
        verify_native_regions: false,
        disable_buffer_fast_path: false,
        namespace_imports: Vec::new(),
        namespace_member_nested: Vec::new(),
        constructor_param_counts: Default::default(),
        imported_classes: Vec::new(),
        short_spread_method_candidates: std::sync::Arc::default(),
        program_class_accessor_names: Default::default(),
        object_literal_method_candidates: std::sync::Arc::default(),
        imported_enums: Vec::new(),
        imported_async_funcs: std::collections::HashSet::new(),
        type_aliases: std::collections::HashMap::new(),
        imported_func_param_counts: std::collections::HashMap::new(),
        imported_func_has_rest: std::collections::HashSet::new(),
        imported_func_synthetic_arguments: std::collections::HashSet::new(),
        imported_func_return_types: std::collections::HashMap::new(),
        imported_vars: std::collections::HashSet::new(),
        output_type: "executable".to_string(),
        disable_constfn_shapes: false,
        program_has_worker: false,
        program_has_thread_agents: false,
        needs_stdlib: false,
        program_is_synchronous: false,
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
        fp_contract_mode: crate::FpContractMode::Off,
        app_metadata: AppMetadata::default(),
        namespace_entries: Vec::new(),
        dynamic_import_path_to_prefix: std::collections::HashMap::new(),
        deferred_module_prefixes: std::collections::HashSet::new(),
        module_init_deps: Vec::new(),
        is_dynamic_import_target: false,
        debug_locations,
        module_source: module_source.map(str::to_string),
        debug_source_line_offset: 0,
    }
}

/// Source whose byte offset 8 (1-based) lands on line 2 (`o.foo;`).
const SRC: &str = "let o;\no.foo;\n";

/// A module whose init reads `o.foo` where `o` is a nullish local — reaching
/// `lower_generic_property_get`. The `PropertyGet` carries a non-zero
/// `byte_offset` exactly as `expr_member/member_tail.rs` now emits for a real
/// `obj.prop` source read.
fn module_with_nullish_read() -> Module {
    let mut m = Module::new("read.ts");
    m.init = vec![
        Stmt::Let {
            id: 1,
            name: "o".to_string(),
            ty: perry_hir::types::Type::Any,
            mutable: false,
            init: Some(Expr::Undefined),
        },
        Stmt::Expr(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(1)),
            property: "foo".to_string(),
            // BytePos 8 → source index 7 ('o' on line 2) → line 2.
            byte_offset: 8,
        }),
    ];
    m.init_kind = ModuleInitKind::Eager;
    m
}

fn emit(debug: bool, source: Option<&str>) -> String {
    String::from_utf8(compile_module(&module_with_nullish_read(), ir_opts(debug, source)).unwrap())
        .expect("LLVM IR should be UTF-8")
}

#[test]
fn imported_variable_read_preserves_class_tags_and_calls_the_live_getter_once() {
    let mut module = Module::new("imported_class_9366.ts");
    module.init.push(Stmt::Expr(Expr::PropertyGet {
        object: Box::new(Expr::ExternFuncRef {
            name: "Renamed".to_string(),
            param_types: vec![],
            return_type: perry_hir::types::Type::Any,
        }),
        property: "prototype".to_string(),
        byte_offset: 0,
    }));
    let mut opts = ir_opts(false, None);
    opts.imported_vars.insert("Renamed".to_string());
    opts.import_function_prefixes
        .insert("Renamed".to_string(), "remote".to_string());
    opts.import_function_origin_names
        .insert("Renamed".to_string(), "Expr".to_string());
    let ir = String::from_utf8(compile_module(&module, opts).unwrap()).unwrap();
    let getter = "perry_fn_remote__Expr";
    assert_eq!(
        ir.matches(&format!("call double @{getter}(")).count(),
        1,
        "{ir}"
    );
    let value = crate::testing::temp_slots::first_call_result(&ir, getter).unwrap();
    // Both classifications may emit their own bitcast of the getter result.
    // Either is intact; a mask or a converted handle would lose its tag.
    let intact_bits: Vec<_> = ir
        .lines()
        .filter_map(|line| {
            let (result, operand) = line.trim().split_once(" = bitcast double ")?;
            (operand == format!("{value} to i64")).then_some(result)
        })
        .collect();
    assert!(
        !intact_bits.is_empty(),
        "getter result must retain its value tag"
    );
    let operands = crate::testing::temp_slots::call_operands(&ir, "js_object_get_field_nonptr")
        .expect("the non-pointer classification arm must remain");
    assert!(
        intact_bits.iter().any(|bits| operands[0] == *bits),
        "the non-pointer arm must receive intact getter bits: {operands:?}\n{ir}"
    );
}

fn emit_guarded_length_read() -> String {
    let mut module = Module::new("guarded_length_read.ts");
    module.init = vec![
        Stmt::Let {
            id: 11,
            name: "values".to_string(),
            ty: perry_hir::types::Type::Array(Box::new(perry_hir::types::Type::Any)),
            mutable: false,
            // An uninitialized erased annotation can still hold any runtime
            // value once control reaches this site. It also prevents scalar
            // replacement from folding the length to a literal.
            init: None,
        },
        Stmt::Return(Some(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(11)),
            property: "length".to_string(),
            byte_offset: 0,
        })),
    ];
    String::from_utf8(compile_module(&module, ir_opts(false, None)).unwrap())
        .expect("LLVM IR should be UTF-8")
}

#[test]
fn property_read_emits_call_location_under_debug_symbols() {
    let ir = emit(true, Some(SRC));
    // Match the CALL, not the always-present `declare` in the runtime preamble.
    assert!(
        ir.contains("call void @js_set_call_location"),
        "expected a js_set_call_location call for the nullish read under \
         --debug-symbols:\n{ir}"
    );
}

#[test]
fn no_call_location_without_debug_symbols() {
    // Default build: debug_locations off → no per-read location call is emitted,
    // keeping release/default output overhead-free.
    let ir = emit(false, None);
    assert!(
        !ir.contains("call void @js_set_call_location"),
        "no js_set_call_location CALL should be emitted without --debug-symbols:\n{ir}"
    );
}

/// Only a live ordinary Array has a header length. Other values retain Get.
#[test]
fn guarded_length_read_has_plain_array_guard_and_exact_fallback() {
    let ir = emit_guarded_length_read();
    let header =
        super::super::class_field_barrier_tests::block_body(&ir, "plen.check_gc.").unwrap();
    assert!(
        header.contains("load i16")
            && header.contains("and i16")
            && header.contains(", 33023")
            && header.contains("icmp eq i16")
            && header.contains(", 1"),
        "kind and forwarding flags must guard the load: {header}"
    );
    let fast = super::super::class_field_barrier_tests::block_body(&ir, "plen.fast.").unwrap();
    assert!(
        fast.contains("load i32") && fast.contains("uitofp i32") && !fast.contains("call "),
        "the proved array reads its live header: {fast}"
    );
    let slow = super::super::class_field_barrier_tests::block_body(&ir, "plen.slow.").unwrap();
    assert!(
        slow.contains("call double @js_value_length_property_f64("),
        "{slow}"
    );
    assert!(
        !ir.contains("@perry_ic_") && !ir.contains("plen.ic."),
        "no removed site state: {ir}"
    );
}

/// A native Uint8Array view normally lowers `.length` to a header load. Once
/// the module can define properties, that load is no longer semantically safe:
/// an own `length` data/accessor property shadows `%TypedArray%.prototype`.
#[test]
fn typed_array_length_uses_property_semantics_after_define_property() {
    let mut module = Module::new("typed_array_length_descriptor.ts");
    module.init = vec![
        Stmt::Let {
            id: 20,
            name: "view".to_string(),
            ty: perry_hir::types::Type::Named("Uint8Array".to_string()),
            mutable: false,
            init: Some(Expr::Uint8ArrayNew(Some(Box::new(Expr::Number(3.0))))),
        },
        Stmt::Expr(Expr::ObjectDefineProperty(
            Box::new(Expr::LocalGet(20)),
            Box::new(Expr::String("length".to_string())),
            Box::new(Expr::Object(vec![])),
        )),
        Stmt::Return(Some(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(20)),
            property: "length".to_string(),
            byte_offset: 0,
        })),
    ];
    let ir = String::from_utf8(compile_module(&module, ir_opts(false, None)).unwrap())
        .expect("LLVM IR should be UTF-8");
    assert!(
        ir.contains("call double @js_value_length_property_key_f64"),
        "a descriptor-capable module must not bypass an own typed-array length:\n{ir}"
    );
}

#[test]
fn typed_array_length_keeps_native_load_without_shape_barrier() {
    let mut module = Module::new("typed_array_length_fast.ts");
    module.init = vec![
        Stmt::Let {
            id: 21,
            name: "view".to_string(),
            ty: perry_hir::types::Type::Named("Uint8Array".to_string()),
            mutable: false,
            init: Some(Expr::Uint8ArrayNew(Some(Box::new(Expr::Number(3.0))))),
        },
        Stmt::Return(Some(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(21)),
            property: "length".to_string(),
            byte_offset: 0,
        })),
    ];
    let ir = String::from_utf8(compile_module(&module, ir_opts(false, None)).unwrap())
        .expect("LLVM IR should be UTF-8");
    assert!(
        ir.contains("load atomic i8, ptr @PERRY_TYPED_NAMED_PROPS_INVALIDATED acquire")
            && ir.contains("load i32")
            && ir.contains("call double @js_value_length_property_key_f64"),
        "a native view should retain its guarded direct load and pooled fallback:\n{ir}"
    );
}

#[test]
fn fs_parent_promises_property_installs_before_resolution() {
    let mut module = Module::new("fs_parent_promises_property.ts");
    module.init = vec![Stmt::Return(Some(Expr::PropertyGet {
        object: Box::new(Expr::NativeModuleRef("fs".to_string())),
        property: "promises".to_string(),
        byte_offset: 0,
    }))];

    let ir = String::from_utf8(compile_module(&module, ir_opts(false, None)).unwrap())
        .expect("LLVM IR should be UTF-8");
    let install = ir
        .find("call void @js_node_submod_install_fs_promises()")
        .unwrap_or_else(|| panic!("fs.promises must emit its submodule installer:\n{ir}"));
    let resolve = ir
        .find("call double @js_native_module_property_by_name")
        .unwrap_or_else(|| {
            panic!("fs.promises must use the native-module property resolver:\n{ir}")
        });
    assert!(
        install < resolve,
        "fs.promises submodule installation must precede property resolution:\n{ir}"
    );
}

#[test]
fn fs_promises_native_module_value_uses_submodule_singleton() {
    let mut module = Module::new("fs_promises_native_module_value.ts");
    module.init = vec![Stmt::Return(Some(Expr::NativeModuleRef(
        "fs/promises".to_string(),
    )))];

    let ir = String::from_utf8(compile_module(&module, ir_opts(false, None)).unwrap())
        .expect("LLVM IR should be UTF-8");
    let install = ir
        .find("call void @js_node_submod_install_fs_promises()")
        .unwrap_or_else(|| panic!("fs/promises must emit its submodule installer:\n{ir}"));
    let namespace = ir
        .find("call double @js_node_submodule_namespace")
        .unwrap_or_else(|| panic!("fs/promises must use its submodule singleton:\n{ir}"));
    assert!(
        install < namespace,
        "fs/promises installation must precede namespace creation:\n{ir}"
    );
}

/// #7189 — `B.ns` where the imported module says `export * as ns from "./m.ts"`.
///
/// The member's value is another module's namespace OBJECT, so there is no
/// `perry_fn_<mod>__ns` symbol for it. Every other namespace-member arm
/// resolves to a symbol, so before this the read fell through to the generic
/// path and produced `undefined` — which is how `z.coerce`, `z.iso`, `z.core`
/// and `z.locales` all came back undefined under zod.
mod nested_namespace_members {
    use super::*;

    fn nested_opts() -> CompileOptions {
        let mut opts = ir_opts(false, None);
        opts.namespace_imports = vec!["B".to_string()];
        opts.namespace_member_prefixes
            .insert(("B".to_string(), "deep".to_string()), "ns2_ts".to_string());
        opts.namespace_member_prefixes
            .insert(("B".to_string(), "gamma".to_string()), "ns3_ts".to_string());
        opts.namespace_member_nested = vec![("B".to_string(), "deep".to_string())];
        opts
    }

    fn module_reading(member: &str) -> Module {
        let mut m = Module::new("nsmain.ts");
        m.init = vec![Stmt::Expr(Expr::PropertyGet {
            object: Box::new(Expr::ExternFuncRef {
                name: "B".to_string(),
                param_types: Vec::new(),
                return_type: perry_hir::types::Type::Any,
            }),
            property: member.to_string(),
            byte_offset: 0,
        })];
        m.init_kind = ModuleInitKind::Eager;
        m
    }

    fn emit_read(member: &str) -> String {
        String::from_utf8(compile_module(&module_reading(member), nested_opts()).unwrap())
            .expect("LLVM IR should be UTF-8")
    }

    /// Slice out the body that actually runs the module's statements.
    ///
    /// Assertions have to be made HERE and not against the whole module. The
    /// declaration pass emits `@__perry_ns_ns2_ts = external` on its own, so a
    /// test that searched the whole IR passed with the read-site fix removed —
    /// it was confirming the declaration existed, not that anything used it.
    ///
    /// For an entry module the statements land in `@main`; `<mod>__init` is an
    /// empty stub. Slicing the stub is its own way of asserting nothing, which
    /// is the mistake this helper exists to avoid making twice.
    fn entry_body(ir: &str) -> String {
        let start = ir.find("define i32 @main()").expect("main must be emitted");
        let end = ir[start..].find("\n}").expect("main must terminate") + start;
        ir[start..end].to_string()
    }

    #[test]
    fn a_nested_namespace_member_loads_the_target_namespace_global() {
        let ir = emit_read("deep");
        let body = entry_body(&ir);
        assert!(
            body.contains("load double, ptr @__perry_ns_ns2_ts"),
            "the read must load the target module's namespace object:\n{body}"
        );
        // The target's init has to run first, or the namespace is read before
        // it has been populated and every member comes back undefined.
        assert!(
            body.contains("call void @ns2_ts__init()"),
            "the target's init must run before its namespace is loaded:\n{body}"
        );
        // The global lives in another module, so this one must declare it or
        // LLVM refuses to parse the IR at all.
        assert!(
            ir.contains("@__perry_ns_ns2_ts = external"),
            "the foreign namespace global must be declared, not just referenced:\n{ir}"
        );
    }

    #[test]
    fn an_ordinary_namespace_member_is_untouched() {
        // The guard against over-reaching: a normal member still resolves the
        // way it always did, through its origin module's symbol rather than a
        // namespace object.
        let body = entry_body(&emit_read("gamma"));
        assert!(
            !body.contains("load double, ptr @__perry_ns_ns3_ts"),
            "a plain member must not be turned into a namespace load:\n{body}"
        );
    }
}

/// A module whose init reads `o.<property>` where `o` is an `Any` local — the
/// generic tower, same shape as `module_with_nullish_read` but with a
/// caller-chosen key.
fn module_reading(property: &str) -> Module {
    let mut m = Module::new("read.ts");
    m.init = vec![
        Stmt::Let {
            id: 1,
            name: "o".to_string(),
            ty: perry_hir::types::Type::Any,
            mutable: false,
            init: Some(Expr::Undefined),
        },
        Stmt::Expr(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(1)),
            property: property.to_string(),
            byte_offset: 0,
        }),
    ];
    m.init_kind = ModuleInitKind::Eager;
    m
}

fn emit_read(property: &str) -> String {
    String::from_utf8(compile_module(&module_reading(property), ir_opts(false, None)).unwrap())
        .expect("LLVM IR should be UTF-8")
}

#[test]
fn generic_non_size_read_has_no_collection_layout_load() {
    let ir = emit_read("other");
    assert!(
        !ir.contains("pget.collection_size") && !ir.contains("pget.collection_kind"),
        "only `.size` may grow the native collection fast path:\n{ir}"
    );
}

#[path = "array_length_tests.rs"]
mod array_length;

/// Unproved byte views, subclass stores and getters use the complete helper.
#[test]
fn guarded_length_declines_non_array_receivers_to_exact_get() {
    let ir = emit_guarded_length_read();
    let header =
        super::super::class_field_barrier_tests::block_body(&ir, "plen.check_gc.").unwrap();
    assert!(
        header.contains("label %plen.fast.") && header.contains("label %plen.slow."),
        "{header}"
    );
    assert!(!ir.contains("plen.byte_header") && !ir.contains("plen.elem.store"));
    assert_eq!(
        ir.matches("call double @js_value_length_property_f64(")
            .count(),
        1
    );
}

#[path = "global_read_tests.rs"]
mod global_read;

#[path = "computed_admission_tests.rs"]
mod computed_admission;

/// A generic named read asks the layout, and only a declined probe runs Get.
#[test]
fn generic_property_read_uses_layout_probe_without_site_state() {
    let ir = emit(false, None);
    assert_eq!(
        ir.matches("call double @perry_probe_get(").count(),
        1,
        "{ir}"
    );
    let probe = ir
        .split("\n\n")
        .find(|block| block.contains("call double @perry_probe_get("))
        .expect("the layout probe's block must exist");
    assert!(
        probe.contains("icmp ne i64") && probe.contains("label %pget.slow."),
        "{probe}"
    );
    let slow = super::super::class_field_barrier_tests::block_body(&ir, "pget.slow.").unwrap();
    assert!(
        slow.contains("call double @js_object_get_field_slow("),
        "{slow}"
    );
    assert!(
        !ir.contains("@perry_ic_") && !ir.contains("@js_object_get_field_ic_front("),
        "{ir}"
    );
}
