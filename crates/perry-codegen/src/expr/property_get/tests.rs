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

fn ir_opts(debug_locations: bool, module_source: Option<&str>) -> CompileOptions {
    CompileOptions {
        target: None,
        is_entry_module: true,
        non_entry_module_prefixes: Vec::new(),
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

/// #6080a: the inline PIC hit predicate must gate raw keys-POINTER tokens on
/// the GC epoch — `cache[2] == @PERRY_IC_EPOCH` — because the `@perry_ic_N`
/// globals are invisible to every GC scanner, so a primed keys-array address
/// that GC frees/moves can be recycled under a different shape and falsely
/// pointer-match. This asserts the emitted IR still carries the guard: the
/// per-site epoch-slot load (gep index 2) and the live-epoch load from the
/// runtime-exported global. Deleting either from `lower_generic_property_get`
/// turns this red.
#[test]
fn generic_property_get_hit_path_is_epoch_gated() {
    let ir = emit(false, None);
    assert!(
        ir.contains("@perry_ic_"),
        "test premise: the generic read reaches the inline monomorphic PIC:\n{ir}"
    );
    assert!(
        ir.contains("load i64, ptr @PERRY_IC_EPOCH"),
        "hit path must load the live read-PIC epoch (@PERRY_IC_EPOCH):\n{ir}"
    );
    // The per-site primed-epoch slot: a gep to index 2 of some @perry_ic_N
    // global (the site number depends on how many IC sites precede this one).
    assert!(
        ir.lines().any(|l| {
            l.contains("getelementptr i64, ptr @perry_ic_") && l.trim_end().ends_with(", i64 2")
        }),
        "hit path must load the per-site primed-epoch slot (cache[2]):\n{ir}"
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

/// #7753, paired with `pic_cache_words_match_codegen` in
/// `perry-runtime/src/object/field_get_set/ic_miss.rs`.
///
/// The runtime writes a `@perry_ic_N` global through `*mut [i64;
/// PIC_CACHE_WORDS]`. If codegen emits a NARROWER global, `pic_prime_get`'s way
/// stores run past the end of it into whatever global the linker placed next —
/// silent memory corruption that no property-read test would notice. This pins
/// the emitted width to the constant both sides share, and pins the constant
/// itself so the runtime's copy cannot drift.
#[test]
fn pic_cache_layout_matches_runtime() {
    use crate::expr::property_get::generic_dispatch::{PIC_CACHE_WORDS, PIC_WAYS, PIC_WAY_BASE};
    assert_eq!(
        PIC_CACHE_WORDS, 12,
        "perry-runtime's PIC_CACHE_WORDS is 12; update both sides together"
    );
    assert!(
        PIC_WAY_BASE + PIC_WAYS * 2 < PIC_CACHE_WORDS,
        "the ways plus the victim counter must fit in the emitted global"
    );
    let ir = emit(false, None);
    assert!(
        ir.contains(&format!(
            "= private global [{PIC_CACHE_WORDS} x i64] zeroinitializer"
        )),
        "every @perry_ic_N must be emitted at the width the runtime writes:\n{ir}"
    );
}

/// #7753: the polymorphic ways must be consulted BEFORE the miss call, and the
/// monomorphic path must not have grown any work.
///
/// A one-entry cache misses on essentially every read at a site whose receiver
/// alternates between shapes — the shape of every discriminated-union dispatch
/// — and each miss runs the full `js_object_get_field_ic_miss` ladder
/// (proxy/closure/buffer/typed-array probes, an accessors thread-local, then a
/// linear keys scan with a `js_string_equals` per key). If the way block is
/// ever deleted or floated below the call it stops paying for itself entirely,
/// and nothing else in the suite would show it — the program still computes the
/// right answer, just slowly. So assert the ORDER, not merely the presence.
#[test]
fn generic_property_get_tries_ways_before_calling_the_miss_handler() {
    let ir = emit(false, None);
    assert!(
        ir.contains("@perry_ic_"),
        "test premise: the generic read reaches the inline PIC:\n{ir}"
    );
    let way_load = ir
        .find("pic.way.load")
        .unwrap_or_else(|| panic!("expected a polymorphic way load block:\n{ir}"));
    let miss_call = ir
        .find("call double @js_object_get_field_ic_miss")
        .unwrap_or_else(|| panic!("expected the miss handler call:\n{ir}"));
    assert!(
        way_load < miss_call,
        "the way compares must be emitted before the miss call, not after it:\n{ir}"
    );
    // The way compares read (token, slot) pairs at words 3.. of the cache.
    use crate::expr::property_get::generic_dispatch::{PIC_WAYS, PIC_WAY_BASE};
    for w in 0..PIC_WAYS {
        for word in [PIC_WAY_BASE + w * 2, PIC_WAY_BASE + w * 2 + 1] {
            assert!(
                ir.contains(&format!("i64 {word})")) || ir.contains(&format!("i64 {word}\n")),
                "way word {word} is never read in the emitted IR:\n{ir}"
            );
        }
    }
}
