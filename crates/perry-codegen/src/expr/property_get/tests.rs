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
        namespace_member_nested: Vec::new(),
        constructor_param_counts: Default::default(),
        imported_classes: Vec::new(),
        short_spread_method_candidates: std::sync::Arc::default(),
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
    let bits = ir
        .lines()
        .find_map(|line| {
            let (result, operand) = line.trim().split_once(" = bitcast double ")?;
            (operand == format!("{value} to i64")).then_some(result)
        })
        .expect("getter result must be classified by its intact value tag");
    // #9366's invariant, one indirection later: T1 moved the INT32 class-ref
    // arm (and its `js_typed_feedback_object_get_field_by_name_f64` call) into
    // `js_object_get_field_ic_nonptr`, which routes on the tag — so the bits it
    // receives must still be the getter's UNMASKED value. A masked handle here
    // would lose the 0x7FFE tag and the class would dispatch as an object.
    assert!(
        ir.lines().any(|line| {
            line.contains("call double @js_object_get_field_ic_nonptr(")
                && line.contains(&format!("i64 {bits},"))
        }),
        "class dispatch must receive the getter's unmasked value bits:\n{ir}"
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

/// #8067: the primary property-read PIC identity is the authoritative ShapeId
/// only. Word 2 may carry the independent Array-subclass named-prefix proof,
/// but it is consulted only after this exact ShapeId predicate fails.
#[test]
fn generic_property_get_hit_path_is_shape_id_only() {
    let ir = emit(false, None);
    assert!(
        ir.contains("@perry_ic_"),
        "test premise: the generic read reaches the inline monomorphic PIC:\n{ir}"
    );
    assert!(
        ir.contains("4611686018427387904"),
        "hit path must form a discriminated ShapeId token:\n{ir}"
    );
    assert!(
        !ir.contains("@PERRY_IC_EPOCH"),
        "the removed pointer-token epoch must not appear in emitted IR:\n{ir}"
    );
}

#[test]
fn guarded_length_read_emits_array_subclass_scalar_ic() {
    let ir = emit_guarded_length_read();
    for block in [
        "plen.ic.header",
        "plen.ic.identity",
        "plen.ic.family_token",
        "plen.ic.inline",
        "plen.ic.spill_load",
    ] {
        assert!(ir.contains(block), "missing {block} from length IC:\n{ir}");
    }
    assert!(
        ir.contains("call double @js_value_length_property_ic_f64"),
        "the cold arm must prime the scalar cache while retaining property semantics:\n{ir}"
    );
    assert!(
        ir.contains("getelementptr i64") && ir.contains("i64 6\n"),
        "the family hit must validate ObjectMeta's named-prefix token:\n{ir}"
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
        ir.contains("call double @js_value_length_property_f64"),
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
        !ir.contains("call double @js_value_length_property_f64"),
        "a barrier-free native view should retain its direct length load:\n{ir}"
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

/// #7753, paired with `pic_cache_words_match_codegen` in
/// `perry-runtime/src/object/field_get_set/ic_miss.rs`.
///
/// The runtime writes a site's cache through `*mut [i64; PIC_CACHE_WORDS]`
/// and the emitted ways read words up to `PIC_WAY_BASE + PIC_WAYS * 2`. Since
/// #9708 the cache words are allocated by the runtime (`pic_slot_resolve`
/// sizes them from its own `PicCache`), so the two constants pinned here are
/// what keeps the emitted way GEPs inside that allocation. The emitted global
/// itself is the 8-byte SLOT, never the words: a `[N x i64]` IC global would
/// be the pre-#9708 shape coming back, with its 96 B of zero-fill per site.
#[test]
fn pic_cache_layout_matches_runtime() {
    use crate::expr::property_get::generic_dispatch::{
        PIC_CACHE_WORDS, PIC_NAMED_PREFIX_TOKEN, PIC_WAYS, PIC_WAY_BASE,
    };
    assert_eq!(
        PIC_CACHE_WORDS, 12,
        "perry-runtime's PIC_CACHE_WORDS is 12; update both sides together"
    );
    assert_eq!(
        PIC_WAY_BASE + PIC_WAYS * 2,
        PIC_CACHE_WORDS,
        "the ways must fill the emitted global exactly"
    );
    assert_eq!(
        PIC_NAMED_PREFIX_TOKEN, 2,
        "runtime PicCache word 2 carries the Array-subclass named-prefix token"
    );
    let ir = emit(false, None);
    let ic_defs: Vec<&str> = ir
        .lines()
        .filter(|l| l.starts_with("@perry_ic_") && l.contains(" = "))
        .collect();
    assert!(
        !ic_defs.is_empty(),
        "test premise: the generic read emits a per-site cache slot:\n{ir}"
    );
    for def in &ic_defs {
        if def.contains("_packed_get =") {
            // NOT zero — see `PACKED_GET_EMPTY`. A zero word would be matched
            // by an unstamped receiver's `parent_class_id`, which is why the
            // hit path used to carry a separate "is this site primed?" test.
            assert!(
                def.ends_with(&format!(
                    " = private global i64 {}, align 8",
                    crate::expr::property_get::generic_dispatch::PACKED_GET_EMPTY
                )),
                "{def}"
            );
            continue;
        }
        assert!(
            def.ends_with(" = private global ptr null"),
            "every @perry_ic_N must be an 8-byte null pointer slot the runtime \
             fills on the first prime (#9708), got:\n{def}\n\nIR:\n{ir}"
        );
    }
    assert!(
        ir.contains("load ptr, ptr @perry_ic_") && ir.contains("icmp ne ptr "),
        "the full-cache fallback must prove the slot non-null before reading \
         a cache word:\n{ir}"
    );
}

/// Object-backed Array subclasses mint one ShapeId per numeric tail length, so
/// a named-field site on such a receiver is served from the independently
/// proved class prefix rather than the exact ShapeId.
///
/// Renamed from `generic_property_get_emits_array_subclass_named_prefix_guard`
/// (T1): the proof itself is unchanged and still runs on exactly the same two
/// words, but it runs in `js_object_get_field_ic_slow` instead of in FOUR
/// emitted blocks per site (`pic.prefix.*`) plus FOUR more for the
/// descriptor-bearing twin (`pic.desc.prefix.*`). Its behaviour is pinned by
/// `an_armed_named_prefix_serves_the_cached_slot` in
/// `perry-runtime/src/object/field_get_set/ic_miss/ic_slow.rs`; what this test
/// keeps is the CODEGEN half of the contract — the emitted site must hand the
/// runtime the two operands that proof reads, and must not have grown its own
/// copy back.
#[test]
fn array_subclass_named_prefix_proof_is_reached_through_the_one_exit() {
    let ir = emit(false, None);
    for gone in [
        "pic.prefix.guard",
        "pic.prefix.meta",
        "pic.prefix.token",
        "pic.prefix.hit",
        "pic.desc.classify",
        "pic.desc.prefix.guard",
        "pic.desc.prefix.meta",
        "pic.desc.prefix.token",
        "pic.desc.prefix.hit",
    ] {
        assert!(
            !ir.contains(gone),
            "the named-prefix ladder must not be emitted per site any more, \
             found `{gone}`:\n{ir}"
        );
    }
    // The runtime half reads cache word 2 against ObjectMeta word 6 and then
    // the cached slot, so the emitted site has to hand it both per-site
    // globals — a call that lost either operand would silently stop serving
    // Array-subclass named fields and fall back to the full lookup.
    let call = ir
        .find("\npic.miss.call")
        .unwrap_or_else(|| panic!("expected the single slow-exit block:\n{ir}"));
    let call_line = ir[call..]
        .lines()
        .find(|l| l.contains("@js_object_get_field_ic_slow("))
        .unwrap_or_else(|| panic!("expected the one slow call:\n{ir}"));
    assert!(
        call_line.contains("ptr @perry_ic_") && call_line.contains("_packed_get"),
        "the slow exit must receive BOTH the cache slot (which holds the \
         named-prefix token in word 2) and the packed MRU word, or the runtime \
         cannot reproduce the arms this site stopped emitting:\n{call_line}"
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
    use crate::expr::property_get::generic_dispatch::{PIC_WAYS, PIC_WAY_BASE, PIC_WAY_STATE};

    // Block *text* order is an artifact of emission order, so assert the CFG
    // instead: the block that calls the miss handler must be reachable only as
    // a branch target of the way block, never straight-line after it.
    let ways = ir
        .find("\npic.ways")
        .unwrap_or_else(|| panic!("expected a pic.ways block:\n{ir}"));
    let way_load = ir
        .find("\npic.way.load")
        .unwrap_or_else(|| panic!("expected a pic.way.load block:\n{ir}"));
    let call_block = ir
        .find("\npic.miss.call")
        .unwrap_or_else(|| panic!("expected a pic.miss.call block:\n{ir}"));
    let ways_body = &ir[ways..[way_load, call_block, ir.len()]
        .into_iter()
        .filter(|&x| x > ways)
        .min()
        .unwrap()];
    assert!(
        ways_body.contains("pic.way.load") && ways_body.contains("pic.miss.call"),
        "pic.ways must end in a branch choosing between the way load and the \
         miss call — otherwise the compares are not gating anything:\n{ways_body}"
    );
    assert!(
        !ways_body.contains("call double @js_object_get_field_ic"),
        "the slow call must not sit inside the way block:\n{ways_body}"
    );
    // The way compares read (token, slot) pairs at words PIC_WAY_BASE.. and the
    // gate reads the state word — all inside pic.ways, none anywhere else.
    for w in 0..PIC_WAYS {
        for word in [PIC_WAY_BASE + w * 2, PIC_WAY_BASE + w * 2 + 1] {
            assert!(
                ways_body.contains(&format!("i64 {word}\n")),
                "way word {word} is never read in the way block:\n{ways_body}"
            );
        }
    }
    assert!(
        ir.contains(&format!("i64 {PIC_WAY_STATE}\n")),
        "the megamorphic gate must read the way-state word:\n{ir}"
    );
}

/// #7907: `pic.miss` must be DOMINATED by `pic.token`, so the way compares can
/// use the values that block already computed instead of re-deriving them.
///
/// #7883 routed all four failure edges — small-handle receiver, non-object
/// receiver, MRU token mismatch, cached slot out of bounds — into one block,
/// which left `token` / `token_nonnull` / `shape_id_eq` live on only some of them
/// and forced the block to reload the whole header ladder. That block is not
/// cold: on a receiver rotation wider than the MRU entry it runs on nearly
/// every read, so the duplicate ladder was hot code. The fix is purely
/// structural — send the two receiver-validation failures to `pic.miss.cold`
/// (they can never resolve a way, since `way_hit` requires a real object) and
/// the dominance follows.
///
/// Assert the *consequences*, not the block names alone: a re-derivation would
/// show up as duplicate header loads or the small-handle sentinel `select`.
#[test]
fn pic_miss_reuses_the_token_blocks_values_instead_of_re_deriving_them() {
    let ir = emit(false, None);
    let main_start = ir
        .find("define i32 @main()")
        .expect("entry module should define main");
    let main_rest = &ir[main_start..];
    let main_end = main_rest
        .find("\n}\n")
        .expect("main should have a closing brace");
    let main = &main_rest[..main_end];
    assert!(
        main.contains("@perry_ic_"),
        "test premise: the generic read reaches the inline PIC:\n{ir}"
    );
    // T1: the landing block is now the single slow exit itself, and the
    // dominance is structural — `pic.miss` has exactly ONE predecessor,
    // `pic.token.miss`, which `pic.token` dominates. Assert that directly:
    // routing any receiver-validation failure back into `pic.miss` would add a
    // predecessor and immediately re-introduce the phis #7907 removed.
    // `pic.miss` carries a numeric suffix and `pic.miss.call` starts with the
    // same text, so match the block's own label exactly and then count the
    // branches whose TARGET is that label (a `br i1` naming both blocks counts
    // once, for the right one).
    let miss_label = main
        .lines()
        .filter(|l| !l.starts_with(' ') && l.ends_with(':'))
        .map(|l| l.trim_end_matches(':'))
        .find(|l| {
            l.strip_prefix("pic.miss.")
                .is_some_and(|tail| tail.chars().all(|c| c.is_ascii_digit()))
        })
        .unwrap_or_else(|| panic!("expected a pic.miss block:\n{ir}"))
        .to_string();
    let preds = main
        .lines()
        .filter(|l| l.trim_start().starts_with("br "))
        .filter(|l| {
            l.split("label %")
                .skip(1)
                .any(|t| t.trim_end_matches(&[',', ' '][..]) == miss_label)
        })
        .count();
    assert_eq!(
        preds, 1,
        "pic.miss must have exactly one predecessor (pic.token.miss), or it is \
         no longer dominated by pic.token:\n{ir}"
    );
    assert!(
        main.contains("label %pic.miss.call"),
        "every receiver-validation failure must land on the single slow \
         exit:\n{ir}"
    );
    assert!(
        !main.contains("@PERRY_IC_EPOCH"),
        "the removed keys-pointer epoch global must not appear:\n{ir}"
    );
    assert!(
        !main.contains("ptrtoint ptr @perry_ic_"),
        "the small-handle sentinel select only existed because an invalid \
         receiver could reach the way compares; it must be gone:\n{ir}"
    );
    // The receiver predicates, exactly once each. `icmp eq i32 %` is two: the
    // ShapeId identity compare on the hit path and the spill compare in
    // `pic.token.miss` that replaced the hit path's overflow-bit test. There
    // is no GC-kind compare at all any more (#10828), so a single `icmp eq
    // i8` would mean the header load has crept back somewhere.
    for (needle, what, expect) in [
        ("icmp eq i8 ", "the GC_TYPE_OBJECT compare", 0),
        ("icmp eq i32 %", "the ShapeId identity compare", 2),
    ] {
        let n = main.matches(needle).count();
        assert_eq!(
            n, expect,
            "{what} appears {n} times, expected {expect} — a receiver \
             predicate is being re-derived or has crept back:\n{ir}"
        );
    }
    // The ONE re-derivation that is deliberate: `pic.token.miss` re-reads the
    // ShapeId word through an atomic load rather than reusing the hot load's
    // value, so that the hot load has a single use and isel folds it into
    // the compare (`cmp %ecx, 4(%rdi)`). A plain second load would be merged
    // back into the first by GVN and the hot word would be live into the
    // cold blocks again.
    let token_miss = main
        .find("\npic.token.miss")
        .unwrap_or_else(|| panic!("expected a pic.token.miss block:\n{ir}"));
    let token_miss_body = &main[token_miss
        ..main[token_miss + 1..]
            .find("\npic.")
            .map(|o| o + token_miss + 1)
            .unwrap_or(main.len())];
    assert!(
        token_miss_body.contains("load atomic i32"),
        "pic.token.miss must re-read the ShapeId word atomically so the hot \
         load stays single-use:\n{token_miss_body}"
    );
}

/// #8067: an exact ShapeId match proves the cached slot's descriptor facts, so
/// the hit path must not reload the compatibility `field_count` mirror merely
/// to re-prove the slot bound.
#[test]
fn cached_slot_bound_comes_from_the_shape_descriptor_match() {
    let floor = crate::target_layout::INLINE_SLOT_FLOOR_LIT;
    let ir = emit(false, None);
    assert!(
        ir.contains("4611686018427387904") && ir.contains("@perry_ic_"),
        "test premise: the emitted read uses a ShapeId PIC:\n{ir}"
    );
    assert!(
        !ir.lines()
            .any(|line| line.contains("icmp ult i64 ") && line.ends_with(&format!(", {floor}")))
            && !ir.contains(&format!(", i64 {floor}, i64 %")),
        "the ShapeId hit path must not materialize a header slot bound:\n{ir}"
    );
}

/// #7907: the way `(token, slot)` reduction is a balanced tree, so the slot
/// select chain is `log2(PIC_WAYS)` deep instead of `PIC_WAYS` deep. Its last
/// node feeds the bounds compare that gates the branch out of `pic.ways`, so
/// the chain depth is directly on the critical path.
///
/// At most one way can hold a given token — `pic_prime_get` evicts a duplicate
/// before writing one, and a zero token is excluded by `token_nonnull` — so
/// reassociating is value-preserving.
#[test]
fn way_slot_reduction_is_a_balanced_tree() {
    use crate::expr::property_get::generic_dispatch::PIC_WAYS;
    let ir = emit(false, None);
    let ways = ir
        .find("\npic.ways")
        .unwrap_or_else(|| panic!("expected a pic.ways block:\n{ir}"));
    // Block labels carry a numeric suffix (`pic.ways.16:`), so the search for
    // the NEXT block has to start past this one's own label or it matches
    // itself and slices an empty body — which reads as "the tree is missing".
    let end = ir[ways + 1..]
        .find("\npic.")
        .map(|o| o + ways + 1)
        .unwrap_or(ir.len());
    let body = &ir[ways..end];
    // A left fold emits PIC_WAYS selects whose 3rd operand is the previous
    // select; the tree emits PIC_WAYS lane selects against the literal 0 plus
    // PIC_WAYS-1 merges. Count the "select against 0" lanes: a fold has one.
    let lanes = body.matches(", i64 0\n").count();
    assert_eq!(
        lanes, PIC_WAYS,
        "expected one `select … , i64 <slot>, i64 0` per way (a balanced tree); \
         a left fold produces exactly one:\n{body}"
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

/// #7883: the inline PIC's guard chain is a chain of BRANCHES, not one flat
/// `and`, so a presence assertion on the individual predicates is no longer
/// evidence of anything — hard-wiring any of the branches to `true` leaves
/// every predicate in the IR as dead code and a "the mask is emitted" test
/// stays green (round 5's first sabotage failed exactly this way).
///
/// This walks the CFG **backwards** from the block that performs the raw
/// inline slot load to the PIC entry, and requires that
///
///   1. every edge on that path is the **true** edge of a `cond_br`
///      (so swapping a branch's successors turns it red), and
///   2. the transitive def chain of those branch conditions contains every
///      guard the raw load depends on for safety (so replacing any condition
///      with a constant, or deleting a predicate, turns it red).
#[test]
fn generic_property_get_slot_load_is_reached_only_through_every_guard() {
    use crate::expr::property_get::generic_dispatch::{PACKED_GET_EMPTY, PACKED_SPILL_FLIP};
    let ir = emit(false, None);

    // Register names restart at %r1 in every function, so the walk MUST be
    // scoped to one function or the def map silently resolves a condition to
    // an identically-named register in a different body (this test read a
    // string-handle `ptrtoint` as the receiver-tag test before it was fixed).
    let func = ir
        .split("\ndefine ")
        .find(|f| f.contains("\npic.hit.") && f.contains("@perry_ic_"))
        .unwrap_or_else(|| panic!("no function contains a PIC hit load:\n{ir}"))
        .to_string();

    let mut blocks: Vec<(String, Vec<String>)> = Vec::new();
    let mut cur: Option<(String, Vec<String>)> = None;
    for line in func.lines() {
        let t = line.trim_end();
        if let Some(lbl) = t.strip_suffix(':') {
            if !lbl.is_empty() && !t.starts_with(' ') && !t.starts_with('\t') {
                if let Some(b) = cur.take() {
                    blocks.push(b);
                }
                cur = Some((lbl.to_string(), Vec::new()));
                continue;
            }
        }
        if let Some((_, body)) = cur.as_mut() {
            body.push(t.to_string());
        }
    }
    if let Some(b) = cur.take() {
        blocks.push(b);
    }
    let load_label = blocks
        .iter()
        .find(|(l, body)| {
            l.starts_with("pic.hit") && body.iter().any(|line| line.contains("load double"))
        })
        .map(|(l, _)| l.clone())
        .unwrap_or_else(|| panic!("no `pic.hit*` block containing a slot load:\n{func}"));

    let mut defs: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for (_, body) in &blocks {
        for l in body {
            if let Some((lhs, rhs)) = l.trim().split_once(" = ") {
                if lhs.starts_with('%') {
                    defs.insert(lhs.to_string(), rhs.to_string());
                }
            }
        }
    }

    // Backwards walk to the entry block, collecting the condition of every
    // `cond_br` whose TRUE edge we arrived on.
    let mut conds: Vec<String> = Vec::new();
    let mut at = load_label.clone();
    let mut steps = 0;
    loop {
        steps += 1;
        assert!(steps < 32, "runaway CFG walk at `{at}`:\n{func}");
        let preds: Vec<&(String, Vec<String>)> = blocks
            .iter()
            .filter(|(_, body)| {
                body.iter().any(|l| {
                    l.trim_start().starts_with("br ") && l.contains(&format!("label %{at}"))
                })
            })
            .collect();
        if preds.is_empty() {
            break; // reached the entry block
        }
        assert_eq!(
            preds.len(),
            1,
            "the guard chain must be a chain — `{at}` has {} predecessors:\n{func}",
            preds.len()
        );
        let (pred_label, pred_body) = preds[0];
        let term = pred_body
            .iter()
            .rev()
            .find(|l| l.trim_start().starts_with("br "))
            .unwrap_or_else(|| panic!("`{pred_label}` has no terminator:\n{func}"));
        let t = term.trim();
        if let Some(rest) = t.strip_prefix("br i1 ") {
            let parts: Vec<&str> = rest.split(", ").collect();
            assert_eq!(parts.len(), 3, "malformed cond_br in `{pred_label}`: {t}");
            let cond = parts[0].to_string();
            let true_target = parts[1].trim_start_matches("label %").to_string();
            assert_eq!(
                true_target, at,
                "`{pred_label}` must reach `{at}` on its TRUE edge — a swapped \
                 cond_br would run the inline slot load when the guard FAILS:\n{t}"
            );
            assert!(
                cond.starts_with('%'),
                "`{pred_label}`'s branch condition is the constant `{cond}` — the \
                 guard decides nothing:\n{func}"
            );
            conds.push(cond);
        }
        at = pred_label.clone();
    }
    assert!(
        conds.len() >= 3,
        "expected at least three guard branches between the PIC entry and the \
         inline slot load, found {}: {conds:?}\n{func}",
        conds.len()
    );

    // Transitive def closure of every collected condition.
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut reached: Vec<String> = Vec::new();
    let mut work = conds.clone();
    while let Some(v) = work.pop() {
        if !seen.insert(v.clone()) {
            continue;
        }
        let Some(rhs) = defs.get(&v) else { continue };
        reached.push(rhs.clone());
        let chars: Vec<char> = rhs.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '%' {
                let mut j = i + 1;
                while j < chars.len()
                    && (chars[j].is_alphanumeric() || chars[j] == '.' || chars[j] == '_')
                {
                    j += 1;
                }
                work.push(chars[i..j].iter().collect());
                i = j;
            } else {
                i += 1;
            }
        }
    }
    let chain = reached.join("\n");
    let packed = defs
        .iter()
        .find(|(_, rhs)| rhs.starts_with("load atomic i64") && rhs.contains("_packed_get"))
        .map(|(reg, _)| reg)
        .expect("compact MRU load");
    // "Has this site primed?" is answered BY the ShapeId compare, not by a
    // test of its own: the site's word is born holding `PACKED_GET_EMPTY`, a
    // value the word at `+4` of a receiver cannot hold. That word is either a
    // ShapeId ([0x8000_0000, 0xC000_0000)), a synthetic class id (at or above
    // 0x8000_0000 today, [0xC000_0000, 0xFFFF_0000) under #10824) or an
    // ordinary HIR class id, which is a counter from 1 — so 0xFFFF_FFFF is
    // above every one of them under BOTH id schemes.
    //
    // This replaces the old `icmp ne i64 %packed, 0` assertion. It is not a
    // weakening: that assertion proved a guard existed, and these three prove
    // the guard is UNNECESSARY — the sentinel is emitted, it is out of range,
    // and the compare that subsumes it still gates the load. Re-introducing a
    // zero initializer turns the first one red.
    assert!(
        ir.contains(&format!(
            "_packed_get = private global i64 {PACKED_GET_EMPTY}, align 8"
        )),
        "the compact MRU must be born holding PACKED_GET_EMPTY, not zero:\n{ir}"
    );
    assert!(
        !(0x8000_0000..0xC000_0000).contains(&PACKED_GET_EMPTY),
        "PACKED_GET_EMPTY must sit outside the ShapeId range so no stamped \
         receiver's shape word can equal an unprimed site"
    );
    assert!(
        !(0x4000_0000..0x8000_0000).contains(&PACKED_GET_EMPTY),
        "and outside the band a SPILL entry is flipped into, or an unprimed \
         site would be decoded as one"
    );
    assert_eq!(
        PACKED_GET_EMPTY, 0xFFFF_FFFF,
        "and above every class id: synthetic ids are at or above 0x8000_0000 \
         today and [0xC000_0000, 0xFFFF_0000) under #10824, and an ordinary \
         HIR class id is a counter from 1"
    );
    assert!(
        chain.contains(&format!("trunc i64 {packed} to i32")),
        "the exact packed ShapeId must gate the field load: {chain}"
    );
    // The overflow-bit test is no longer a guard on the inline load: a
    // SPILL-located key publishes its ShapeId with PACKED_SPILL_FLIP flipped
    // in, which lands it in [0x4000_0000, 0x8000_0000) — neither a ShapeId nor
    // any class id — so the compare above refuses it without a question of its
    // own. If the bit test comes back it is 10 bytes of `movabs`, a `test` and
    // a branch on every read.
    assert!(
        !chain.contains(&PACKED_SPILL_FLIP.to_string()),
        "the overflow-bit test must not gate the inline slot load — a spill \
         entry is refused by the ShapeId compare itself:\n{chain}"
    );
    // The inherited-read hook (#10834/#10842) lives on the DECLINED edge. Its
    // answer must never be a condition on the way to the own slot load: if it
    // were, an own read would pay a call, and this walk would have collected
    // the call's result in the chain.
    assert!(
        !chain.contains("js_inherited_read_cache_hit_f64"),
        "the inherited-read hook must not gate the inline slot load:\n{chain}"
    );

    // The GC header is not read on the way to the slot load at all: neither
    // the kind byte (#10828 closed rule 3 — a `+4` word equal to a live
    // ShapeId proves `GC_TYPE_OBJECT`) nor the descriptor flag (#10824 closed
    // rule 1 — every descriptor change transitions the ShapeId). The chain is
    // therefore EXACTLY three branches: the receiver-tag test, the
    // small-handle test and the ShapeId compare. Each retired predicate is
    // asserted absent from the WHOLE function, not merely off the chain, or
    // it could be tested somewhere the walk does not see.
    assert_eq!(
        conds.len(),
        3,
        "the guard chain must be exactly tag test, small-handle test and \
         ShapeId compare, found {conds:?}\n{func}"
    );
    assert!(
        !defs
            .values()
            .any(|rhs| rhs.starts_with("icmp eq i8 %") && rhs.ends_with(", 2")),
        "the GC_TYPE_OBJECT kind compare must not be emitted — the ShapeId \
         compare proves the kind since #10828:\n{func}"
    );
    for (gone, what) in [
        (", 134217983", "the packed kind+descriptor mask"),
        (", 2048", "the OBJ_FLAG_HAS_DESCRIPTORS mask"),
        ("load i16", "the reserved-halfword load"),
        ("load i8", "the GC-kind byte load"),
    ] {
        assert!(
            !func.contains(gone),
            "{what} must not be emitted any more — kind and descriptor state \
             are shape-carried since #10824/#10828 (found `{gone}`):\n{func}"
        );
    }

    for (needle, what) in [
        // The exact POINTER test is `(bits ^ POINTER_TAG) >> 48 == 0`, on the
        // value the pointer path then uses as its handle; the tag constant is
        // the xor's operand.
        (
            crate::nanbox::POINTER_TAG_I64,
            "the POINTER receiver-tag test",
        ),
        ("1048575", "the small-handle (native registry id) test"),
        ("@perry_ic_", "the per-site cached shape-token compare"),
    ] {
        assert!(
            chain.contains(needle),
            "the inline slot load must be gated on {what}, but no branch \
             condition on the path to `{load_label}` depends on it.\n\
             conditions: {conds:?}\nreached def chain:\n{chain}\n\nIR:\n{func}"
        );
    }

    // The loaded value is the answer: no `TAG_HOLE` compare follows the slot
    // load in the hit block. #10826 made every successful delete a shape
    // transition, so a ShapeId hit proves the slot it names is live, and the
    // four-instruction hole check was the patch for exactly that operation.
    // The compare is asserted PRESENT on the way path, which keeps it: that
    // pins that the constant did not merely vanish from the IR, and that the
    // way path's removal is the separate, measured change the emitter says
    // it is.
    let hit_body = blocks
        .iter()
        .find(|(l, _)| *l == load_label)
        .map(|(_, body)| body.join("\n"))
        .expect("the hit block was found above");
    assert!(
        !hit_body.contains(crate::nanbox::TAG_HOLE_I64),
        "the hit block must not compare the loaded slot against TAG_HOLE — a \
         ShapeId hit proves the slot live since #10826:\n{hit_body}"
    );
    assert!(
        hit_body.contains("load double") && hit_body.contains("br label %"),
        "the hit block must end in the slot load and an unconditional branch \
         to the merge:\n{hit_body}"
    );
    let way_body = blocks
        .iter()
        .find(|(l, _)| l.starts_with("pic.way.load"))
        .map(|(_, body)| body.join("\n"))
        .expect("the way load block");
    assert!(
        way_body.contains(crate::nanbox::TAG_HOLE_I64),
        "the way path keeps its TAG_HOLE compare (removing it is a separate \
         change):\n{way_body}"
    );
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

/// A `.length` read whose receiver codegen cannot prove is a string must still
/// serve a string inline.
///
/// The proven-string lowering in `property_get.rs` already emits a
/// runtime-guarded three-arm dispatch, but it is gated on `is_string_expr` — a
/// compile-time proof. Without a proof the read lands in this tower, where a
/// heap string can never hit the PIC (it requires a GC_TYPE_OBJECT receiver by
/// construction, #72) and every read pays the full
/// `js_object_get_field_ic_miss` object ladder. Assert BOTH string arms exist:
/// the heap block, and the SSO arm's inline length-byte extract in place of the
/// `js_object_get_field_by_name_f64` call.
#[test]
fn generic_length_read_serves_a_string_inline() {
    let ir = emit_read("length");
    assert!(
        ir.contains("\npget.strlen_heap"),
        "a `.length` read must split heap strings off before the PIC:\n{ir}"
    );
    // 32767 = STRING_TAG >> 48. The split must test the tag, not something the
    // optimiser could fold away.
    assert!(
        ir.contains("icmp eq i64") && ir.contains("32767"),
        "the heap-string split must compare the receiver tag to STRING_TAG:\n{ir}"
    );
    let sso = ir
        .find("\npget.recv_sso")
        .unwrap_or_else(|| panic!("expected an SSO receiver block:\n{ir}"));
    let sso_body = &ir[sso..];
    let sso_end = sso_body[1..]
        .find("\n\n")
        .map(|i| i + 1)
        .unwrap_or(sso_body.len());
    let sso_body = &sso_body[..sso_end];
    assert!(
        ir.contains("\nsso.utf16") && ir.contains("\nsso.length.done"),
        "non-ASCII inline strings must have a UTF-16 counting arm:\n{ir}"
    );
    assert!(
        sso_body.contains("lshr i64") && sso_body.contains(", 40"),
        "the SSO arm must extract the inline length byte, not call the \
         by-name helper:\n{sso_body}"
    );
    assert!(
        !sso_body.contains("js_object_get_field_by_name_f64"),
        "the SSO `.length` arm must not call back into the runtime:\n{sso_body}"
    );
    // Everything that is NOT a string keeps the tower.
    assert!(
        ir.contains("@perry_ic_") && ir.contains("js_object_get_field_ic_slow"),
        "non-string receivers must still reach the inline PIC and its slow \
         exit:\n{ir}"
    );
}

/// The short-circuit is keyed on the property name: any other key on a string
/// receiver (`s.charCodeAt`, `s.constructor`) still needs the runtime.
///
/// T1 moved the SSO arm itself behind the one exit — an SSO receiver with any
/// key but `length` is served by `js_object_get_field_ic_slow`'s tag ladder
/// (`sso_receiver_routes_to_the_by_name_helper` in
/// `ic_miss/ic_slow.rs` pins that it still reaches the by-name helper). What
/// codegen must guarantee is that such a receiver LEAVES: it must never fall
/// into the PIC, whose header loads would read the SSO payload as an address.
#[test]
fn generic_non_length_read_keeps_the_whole_tower() {
    let ir = emit_read("charCodeAt");
    for gone in ["pget.strlen_heap", "pget.recv_sso"] {
        assert!(
            !ir.contains(gone),
            "only `.length` may grow an inline string arm, found `{gone}`:\n{ir}"
        );
    }
    // The receiver-tag test is the one test that decides whether the receiver
    // may be dereferenced at all, and for every key but `.length` it is the
    // EXACT POINTER test, spelled `(bits ^ POINTER_TAG) >> 48 == 0` on the
    // value that becomes the handle. Its false edge must be the NON-POINTER
    // exit — a distinct block with a distinct callee, which is what stops
    // SimplifyCFG folding this guard into the next one.
    let branch = ir
        .lines()
        .find(|l| l.trim_start().starts_with("br i1 ") && l.contains("label %pget.recv_other"))
        .unwrap_or_else(|| panic!("expected a branch to the non-pointer exit:\n{ir}"));
    let cond = branch
        .trim()
        .strip_prefix("br i1 ")
        .and_then(|rest| rest.split_once(','))
        .map(|(c, _)| c.to_string())
        .unwrap_or_else(|| panic!("malformed branch: {branch}"));
    let tag_test = ir
        .lines()
        .find(|l| l.trim().starts_with(&format!("{cond} = ")))
        .unwrap_or_else(|| panic!("expected the receiver-tag test defining {cond}:\n{ir}"));
    assert!(
        tag_test.contains("icmp eq i64 ") && tag_test.trim_end().ends_with(", 0"),
        "the exact POINTER test compares the xor-ed tag half-word to zero:\n{tag_test}"
    );
    assert!(
        ir.contains("xor i64 %") && ir.contains(crate::nanbox::POINTER_TAG_I64),
        "the handle must be `bits ^ POINTER_TAG`, the value the tag test is \
         computed from:\n{ir}"
    );
    assert!(
        branch.contains("label %pget.recv_other") && !branch.contains("label %pic.miss.call"),
        "a non-pointer receiver must leave for its OWN exit — sharing the \
         object exit's block is what cost +4 instructions per hit:\n{branch}"
    );
    assert!(
        ir.contains("@js_object_get_field_ic_slow(")
            && ir.contains("@js_object_get_field_ic_nonptr("),
        "the tower must still reach both slow entries:\n{ir}"
    );
}

/// A dynamically typed `.size` read must recognize native Map/Set receivers
/// by their live GC kinds before entering the object-only PIC. This covers
/// nested structural reads such as `this.ctx.hooks.size` without trusting an
/// erased TypeScript annotation as a native-layout proof.
#[test]
fn generic_size_read_serves_native_collections_inline() {
    let ir = emit_read("size");
    let collection = ir
        .find("\npget.collection_size")
        .unwrap_or_else(|| panic!("expected a native collection size block:\n{ir}"));
    let collection_body = &ir[collection..];
    let collection_end = collection_body[1..]
        .find("\n\n")
        .map(|i| i + 1)
        .unwrap_or(collection_body.len());
    let collection_body = &collection_body[..collection_end];

    assert!(
        ir.contains("icmp eq i8") && ir.contains(", 8") && ir.contains(", 12"),
        "the collection arm must be guarded by GC_TYPE_MAP and GC_TYPE_SET:\n{ir}"
    );
    assert!(
        collection_body.contains("load i32") && collection_body.contains("uitofp i32"),
        "the branded collection arm must load the shared leading size field inline:\n\
         {collection_body}"
    );
    assert!(
        ir.contains("@perry_ic_") && ir.contains("js_object_get_field_ic_slow"),
        "non-collection receivers must retain the generic property tower:\n{ir}"
    );
}

#[test]
fn generic_non_size_read_has_no_collection_layout_load() {
    let ir = emit_read("other");
    assert!(
        !ir.contains("pget.collection_size") && !ir.contains("pget.collection_kind"),
        "only `.size` may grow the native collection fast path:\n{ir}"
    );
}

/// The object-backed `.length` tier probes the elements-backed subclass store
/// first: meta word → `ObjectMeta.elements` (word 12) → the inner Array's
/// `length` word — and only then the shape/family IC.
#[test]
fn the_length_tier_probes_the_elements_store_before_the_shape_ic() {
    let ir = emit_guarded_length_read();
    assert!(
        ir.contains("plen.elem.meta") && ir.contains("plen.elem.length"),
        "the elements probe must exist:\n{ir}"
    );
    let store = super::super::class_field_barrier_tests::block_body(&ir, "plen.elem.store.")
        .expect("the elements-store probe block exists");
    assert!(
        store.contains("getelementptr i64, ptr %") && store.contains(", i64 12"),
        "the probe must load ObjectMeta.elements at word 12:\n{store}"
    );
    // A miss of the probe keeps the shape IC.
    assert!(
        store.contains("plen.ic.shape"),
        "a missing store must fall through to the shape IC:\n{store}"
    );
}

/// The GC header is not read by a generic property read on ANY target: the
/// kind byte is proved by the ShapeId compare (#10828, rule 3) and the
/// descriptor flag is shape-carried (#10824, rule 1). With the header load
/// went the only reason this tower ever cared about endianness — the packed
/// `i32` kind+descriptor word on little-endian targets versus the byte +
/// `i16` reserved-halfword pair elsewhere.
///
/// Renamed from `packed_pic_header_guard_is_endianness_aware`: that test
/// pinned the packed mask's PRESENCE on x86-64/aarch64, which is now the
/// regression this one exists to catch.
#[test]
fn no_gc_header_load_on_any_target() {
    for target in [
        "aarch64-apple-darwin",
        "x86_64-unknown-linux-gnu",
        "powerpc64-unknown-linux-gnu",
    ] {
        let mut opts = ir_opts(false, None);
        opts.target = Some(target.to_string());
        let ir =
            String::from_utf8(compile_module(&module_with_nullish_read(), opts).unwrap()).unwrap();
        let main = ir
            .split("\ndefine ")
            .find(|f| f.contains("\npic.token"))
            .unwrap_or_else(|| panic!("{target}: no function contains the tower:\n{ir}"));
        for (gone, what) in [
            (", 134217983", "the packed kind+descriptor mask"),
            (", 2048", "the OBJ_FLAG_HAS_DESCRIPTORS mask"),
            ("load i16", "the reserved-halfword load"),
            ("load i8", "the GC-kind byte load"),
            ("icmp eq i8", "the GC-kind compare"),
        ] {
            assert!(
                !main.contains(gone),
                "{target}: {what} must not be emitted (found `{gone}`):\n{main}"
            );
        }
        // The tower still ends in the one exit: a receiver that is not a
        // shaped ordinary object, or one whose descriptor install transitioned
        // its ShapeId, reaches it by FAILING THE SHAPE COMPARE, and the runtime
        // keeps the Array-subclass named-prefix exception behind it.
        assert!(
            main.contains("@js_object_get_field_ic_slow(") && main.contains("\npic.miss.call"),
            "{target}: the slow exit must remain:\n{main}"
        );
    }
}

#[test]
fn compact_get_mru_is_atomic_and_full_cache_remains_lazy() {
    use crate::expr::property_get::generic_dispatch::PACKED_GET_EMPTY;
    let ir = emit(false, None);
    assert!(
        ir.contains(&format!(
            "_packed_get = private global i64 {PACKED_GET_EMPTY}, align 8"
        )),
        "{ir}"
    );
    assert!(
        ir.contains("load atomic i64") && ir.contains("monotonic, align 8"),
        "{ir}"
    );
    assert!(ir.contains("@js_object_get_field_ic_slow("), "{ir}");
    // The `trunc` is the ShapeId half of the compact word. There is no
    // `icmp ne i64 %packed, 0` beside it any more: the sentinel above makes
    // the ShapeId compare prove the site is primed as well. Named by the
    // packed word's register: a blanket "no `icmp ne i64`" would now also
    // forbid the inherited-read hook's decline compare on the exit edge,
    // which is a different question about a different value.
    let packed = ir
        .lines()
        .find(|l| l.contains("load atomic i64") && l.contains("_packed_get"))
        .and_then(|l| l.trim().split_once(" = "))
        .map(|(reg, _)| reg.to_string())
        .expect("the compact MRU load");
    assert!(
        ir.contains("trunc i64")
            && !ir.contains(&format!("icmp ne i64 {packed}, 0"))
            && !ir.contains(&format!("icmp eq i64 {packed}, 0")),
        "the compact word must not be tested against zero:\n{ir}"
    );
    assert!(
        ir.contains("pic.token.miss"),
        "a full-cache dereference must still guard a null site: {ir}"
    );
}

/// T1: the whole point — per untyped `obj.prop` the emitted tower is TWO calls
/// and a handful of blocks, with the inline hit and the polymorphic ways kept.
///
/// This is a ratchet, so it is an EXACT count in both dimensions. The tower it
/// replaced expanded 33 tower blocks and SIX runtime call sites per site
/// (`js_object_get_field_by_name_f64` twice, the feedback-wrapped class-ref
/// helper, `js_throw_type_error_property_access`,
/// `js_object_get_field_ic_overflow_load`, `js_object_get_field_ic_miss_packed`),
/// each one a statepoint whose live GC values are written into
/// `.perry_gcmap`. On @babel/parser that was 29% of all emitted IR over 6,487
/// sites; a single arm creeping back inline is a regression measured in
/// megabytes of `.text`, and nothing else in the suite would report it.
///
/// Two and not one: a single shared exit let SimplifyCFG fold the receiver-tag
/// test and the small-handle test into one flat predicate, costing +4.00
/// instructions on every HIT (measured). The separate non-pointer callee is
/// what keeps that guard chain branchy, so the count below is 2 — and a change
/// that makes it 1 is a hit-path regression, not a size win.
/// A SPILL-located key must still be RECOGNISED — just not on the hit path.
///
/// Taking the overflow-bit test off the hit path is only sound if the entry it
/// used to catch is caught somewhere else. `pic.token.miss` un-flips
/// `PACKED_SPILL_FLIP` and branches straight to the one exit, skipping the
/// full cache's resolution and the ways (neither can hold an encoded slot).
/// Without this test, deleting the spill compare would leave every spill read
/// correct-but-slow — it would walk the ways, miss, call out, and re-scan the
/// keys array on every read, which is invisible in program output.
#[test]
fn a_spill_entry_is_recognised_in_the_token_miss_block_and_nowhere_else() {
    use crate::expr::property_get::generic_dispatch::PACKED_SPILL_FLIP;
    let ir = emit(false, None);
    let func = ir
        .split("\ndefine ")
        .find(|f| f.contains("\npic.token.miss"))
        .unwrap_or_else(|| panic!("no function contains the generic tower:\n{ir}"));

    // Split the function into blocks and find `pic.token.miss`'s body.
    let mut body: Vec<&str> = Vec::new();
    let mut inside = false;
    for line in func.lines() {
        if !line.starts_with(' ') && line.ends_with(':') {
            inside = line.trim_end_matches(':').starts_with("pic.token.miss");
            continue;
        }
        if inside {
            body.push(line);
        }
    }
    let body = body.join("\n");
    assert!(
        body.contains("xor i32 ") && body.contains(&PACKED_SPILL_FLIP.to_string()),
        "`pic.token.miss` must un-flip PACKED_SPILL_FLIP to recognise a spill \
         entry:\n{body}"
    );
    assert!(
        body.contains("pic.miss.call"),
        "a recognised spill entry must branch straight to the one exit, not \
         walk the ways:\n{body}"
    );
}

#[test]
fn the_generic_tower_is_two_calls_and_a_bounded_number_of_blocks() {
    let ir = emit(false, None);
    let func = ir
        .split("\ndefine ")
        .find(|f| f.contains("\npic.miss.call"))
        .unwrap_or_else(|| panic!("no function contains the generic tower:\n{ir}"));

    // Every call/invoke in the whole function, by callee. Feedback records are
    // compile-time gated and absent from this build; anything else must be the
    // one exit (the fixture's module init contributes its own calls, so match
    // on the property-GET family rather than on a total).
    let pget_calls: Vec<&str> = func
        .lines()
        .filter(|l| l.contains(" call ") || l.contains(" invoke "))
        .filter_map(|l| l.split(" @").nth(1))
        .filter_map(|c| c.split('(').next())
        .filter(|c| {
            c.starts_with("js_object_get_field")
                || c.starts_with("js_typed_feedback_object_get_field")
                || *c == "js_throw_type_error_property_access"
        })
        .collect();
    let mut sorted = pget_calls.clone();
    sorted.sort();
    assert_eq!(
        sorted,
        vec![
            "js_object_get_field_ic_nonptr",
            "js_object_get_field_ic_slow"
        ],
        "the tower must expand exactly two property-GET call sites:\n{func}"
    );

    let blocks: Vec<&str> = func
        .lines()
        .filter(|l| !l.starts_with(' ') && l.ends_with(':'))
        .map(|l| l.trim_end_matches(':'))
        .filter(|l| l.starts_with("pget.") || l.starts_with("pic."))
        .collect();
    let mut expected = vec![
        // the guard chain and the inline hit
        "pget.recv_ok",
        // the non-pointer exit, off the tag test's false edge
        "pget.recv_other",
        // `pic.recv_hdr` is GONE: it existed to load the GC header word, and
        // the ShapeId compare in `pic.token` now proves the kind (#10828) and
        // the descriptor state (#10824) that word was loaded for.
        "pic.token",
        "pic.token.miss",
        // The spill entry's landing block. `pic.token.miss` recognises a
        // SPILL-located key by un-flipping PACKED_SPILL_FLIP and branches
        // straight to the one exit; everything else continues here to the full
        // cache and the ways. `pic.hit.inline` is GONE: with spill entries
        // refused by the ShapeId compare itself, the hit block has nothing to
        // decide between and the load sits directly in `pic.hit`.
        "pic.token.ways",
        // The hit block ends in the slot load and a branch to the merge:
        // `pic.hit.deleted` is GONE with the `TAG_HOLE` compare (#10826 made
        // delete a shape transition, so a ShapeId hit proves the slot live);
        // `pic.hit.live` exists only when typed feedback has something to
        // record on the live edge.
        "pic.hit",
        // the polymorphic ways, deliberately still inline (#7753)
        "pic.miss",
        "pic.ways",
        "pic.way.load",
        "pic.way.live",
        // the inherited-read hook, on the never-primed edge out of
        // `pic.token.ways` and nowhere else (`js_inherited_read_cache_hit_f64`,
        // a leaf); a decline continues to the one exit
        "pic.miss.inherited",
        // the one exit, and the join
        "pic.miss.call",
        "pget.recv_merge",
    ];
    // Labels carry a numeric suffix (`pic.ways.16`); strip it for comparison.
    let mut normalized: Vec<String> = blocks
        .iter()
        .map(|b| {
            let mut parts: Vec<&str> = b.split('.').collect();
            if parts.last().is_some_and(|p| p.parse::<u32>().is_ok()) {
                parts.pop();
            }
            parts.join(".")
        })
        .collect();
    normalized.sort();
    expected.sort();
    assert_eq!(
        normalized,
        expected.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        "the emitted tower's block set changed:\n{func}"
    );
}

/// The inherited-read cache (#10834/#10842) is asked on the NEVER-PRIMED edge
/// and nowhere else. A read whose key lives on the prototype chain is never an
/// own slot on the receiver's shape, so a site that only reads such a key never
/// resolves its per-site cache, and every read of it reaches `pic.token.ways`
/// with `present` false. That edge — which used to go straight to the exit —
/// now asks the cache before calling out. The first placement asked on EVERY
/// path into the exit and charged each own-key miss a declining probe (+88 on
/// a megamorphic site, +89 on a spill read, measured); this one costs every
/// other path zero instructions.
///
/// Five things are pinned, each of which would otherwise fail silently (the
/// program still computes the right value through the slow entry):
///
/// 1. the hook call sits in `pic.miss.inherited` and in no other block, in
///    particular NOT on any path to the inline slot load (the CFG-walk test
///    asserts the same from the other side);
/// 2. that block is reached from `pic.token.ways` on the FALSE edge of the
///    cache-present test, and from nowhere else;
/// 3. its result is branched on with the SERVED edge as the true edge, the
///    tower's rule for every guard-passing edge, and the false edge is the
///    one exit;
/// 4. the slow entry is still called from `pic.miss.call` only, with the same
///    four operands;
/// 5. the merge phi takes the served value from `pic.miss.inherited`.
#[test]
fn the_inherited_read_cache_is_asked_on_the_never_primed_edge_only() {
    let ir = emit(false, None);
    let func = ir
        .split("\ndefine ")
        .find(|f| f.contains("\npic.miss.call"))
        .unwrap_or_else(|| panic!("no function contains the generic tower:\n{ir}"));
    let mut blocks: Vec<(String, Vec<String>)> = Vec::new();
    let mut cur: Option<(String, Vec<String>)> = None;
    for line in func.lines() {
        if !line.starts_with(' ') && line.ends_with(':') {
            if let Some(b) = cur.take() {
                blocks.push(b);
            }
            cur = Some((line.trim_end_matches(':').to_string(), Vec::new()));
            continue;
        }
        if let Some((_, body)) = cur.as_mut() {
            body.push(line.trim().to_string());
        }
    }
    if let Some(b) = cur.take() {
        blocks.push(b);
    }
    // 1. one caller block, and it is the inherited arm.
    let holders: Vec<&str> = blocks
        .iter()
        .filter(|(_, body)| {
            body.iter()
                .any(|l| l.contains("call double @js_inherited_read_cache_hit_f64("))
        })
        .map(|(l, _)| l.as_str())
        .collect();
    assert_eq!(
        holders.len(),
        1,
        "the inherited hook must be called from exactly one block: {holders:?}\n{func}"
    );
    let inh_label = holders[0];
    assert!(
        inh_label.starts_with("pic.miss.inherited"),
        "the hook belongs on the never-primed edge, found it in `{inh_label}`:\n{func}"
    );
    let (_, inh_body) = blocks.iter().find(|(l, _)| l == inh_label).unwrap();
    let hook_line = inh_body
        .iter()
        .find(|l| l.contains("@js_inherited_read_cache_hit_f64("))
        .unwrap();
    assert!(
        hook_line.contains("(ptr %") && hook_line.matches(", ptr %").count() == 1,
        "the hook takes the masked receiver and the interned key as two \
         pointers:\n{hook_line}"
    );
    // 2. reached only from `pic.token.ways`, on the FALSE edge of `present`.
    let preds: Vec<(&str, &str)> = blocks
        .iter()
        .flat_map(|(l, body)| {
            body.iter()
                .filter(|t| t.starts_with("br ") && t.contains(&format!("label %{inh_label}")))
                .map(move |t| (l.as_str(), t.as_str()))
        })
        .collect();
    assert_eq!(
        preds.len(),
        1,
        "exactly one edge may reach the hook: {preds:?}\n{func}"
    );
    let (pred_label, pred_term) = preds[0];
    assert!(
        pred_label.starts_with("pic.token.ways"),
        "the hook's one predecessor must be the cache-present test: {pred_label}"
    );
    let parts: Vec<&str> = pred_term
        .strip_prefix("br i1 ")
        .unwrap()
        .split(", ")
        .collect();
    assert!(
        parts[1].starts_with("label %pic.miss") && !parts[1].starts_with("label %pic.miss.inh"),
        "the TRUE edge of `present` must still be the way compares: {pred_term}"
    );
    assert!(
        parts[2].starts_with(&format!("label %{inh_label}")),
        "the hook must sit on the FALSE (never-primed) edge: {pred_term}"
    );
    let present_def = blocks
        .iter()
        .find(|(l, _)| l == pred_label)
        .and_then(|(_, body)| {
            body.iter()
                .find(|l| l.starts_with(&format!("{} = ", parts[0])))
        })
        .unwrap_or_else(|| {
            panic!(
                "the branch condition {} must be defined in {pred_label}",
                parts[0]
            )
        });
    assert!(
        present_def.contains("icmp ne ptr ") && present_def.ends_with(", null"),
        "`present` is the cache slot's non-null test:\n{present_def}"
    );
    // 3. polarity: `icmp ne <bits>, TAG_HOLE` is "served", served is the TRUE
    //    edge and lands on the merge; the false edge is the one exit.
    let served = inh_body
        .iter()
        .find(|l| l.contains("icmp ne i64 ") && l.ends_with(crate::nanbox::TAG_HOLE_I64))
        .unwrap_or_else(|| panic!("the decline compare against TAG_HOLE:\n{func}"));
    let cond = served.split_once(" = ").map(|(c, _)| c).unwrap();
    let term = inh_body
        .iter()
        .rev()
        .find(|l| l.starts_with("br "))
        .unwrap();
    let parts: Vec<&str> = term
        .strip_prefix("br i1 ")
        .unwrap_or_else(|| panic!("the arm must branch on the hook's answer: {term}"))
        .split(", ")
        .collect();
    assert_eq!(
        parts[0], cond,
        "the branch must be on the served predicate: {term}"
    );
    assert!(
        parts[1].starts_with("label %pget.recv_merge"),
        "the SERVED edge must be the true edge and land on the merge: {term}"
    );
    assert!(
        parts[2].starts_with("label %pic.miss.call"),
        "the decline must be the false edge into the one exit: {term}"
    );
    // 4. the slow entry: one caller, the exit, same operands.
    let slow_callers: Vec<&str> = blocks
        .iter()
        .filter(|(_, body)| {
            body.iter()
                .any(|l| l.contains("@js_object_get_field_ic_slow("))
        })
        .map(|(l, _)| l.as_str())
        .collect();
    assert_eq!(slow_callers.len(), 1, "{slow_callers:?}");
    assert!(
        slow_callers[0].starts_with("pic.miss.call"),
        "the slow entry must be called from the one exit: {slow_callers:?}"
    );
    let (_, slow_body) = blocks.iter().find(|(l, _)| l == slow_callers[0]).unwrap();
    let slow_line = slow_body
        .iter()
        .find(|l| l.contains("@js_object_get_field_ic_slow("))
        .unwrap();
    assert!(
        slow_line.contains("ptr @perry_ic_") && slow_line.contains("_packed_get"),
        "the slow entry must still receive the cache slot and the packed \
         word:\n{slow_line}"
    );
    // 5. the merge takes the served value from the inherited arm.
    let (_, merge_body) = blocks
        .iter()
        .find(|(l, _)| l.starts_with("pget.recv_merge"))
        .unwrap();
    let phi = merge_body
        .iter()
        .find(|l| l.contains(" = phi double "))
        .unwrap();
    let served_value = hook_line.split_once(" = ").map(|(v, _)| v).unwrap();
    assert!(
        phi.contains(&format!("[ {served_value}, %{inh_label} ]")),
        "the merge must take the hook's value from `{inh_label}`:\n{phi}"
    );
}
