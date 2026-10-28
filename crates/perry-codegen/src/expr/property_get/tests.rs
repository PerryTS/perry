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

#[path = "front_contract_tests.rs"]
mod front_contract;
#[path = "named_accessor_tests.rs"]
mod named_accessor;
use front_contract::{front_call_block, verify_front_directory};

fn ir_opts(debug_locations: bool, module_source: Option<&str>) -> CompileOptions {
    CompileOptions {
        static_shape_ids: Vec::new(),
        program_layout_ordinals: Default::default(),
        program_class_shape_ids: Default::default(),
        program_facts: Default::default(),
        program_layouts: Default::default(),
        gc_effects: Default::default(),
        target: None,
        is_entry_module: true,
        non_entry_module_prefixes: Vec::new(),
        thread_literal_module_prefixes: Vec::new(),
        nextjs_path_init_modules: Vec::new(),
        lazy_path_init_modules: Vec::new(),
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
        namespace_source_prefixes: Default::default(),
        namespace_member_nested: Vec::new(),
        cjs_interop_namespaces: Vec::new(),
        constructor_param_counts: Default::default(),
        imported_classes: Vec::new(),
        short_spread_method_candidates: std::sync::Arc::default(),
        program_class_accessor_names: Default::default(),
        object_literal_method_candidates: std::sync::Arc::default(),
        imported_enums: Vec::new(),
        imported_async_funcs: std::collections::HashSet::new(),
        type_aliases: Default::default(),
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
        deferred_module_prefixes: Default::default(),
        module_init_deps: Vec::new(),
        is_dynamic_import_target: false,
        namespace_adopts_cjs_exports: false,
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
            byte_end: 0,
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
        byte_end: 0,
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
    // The receiver is classified once for the pointer test and once more as
    // the exit's operand: any bitcast of the getter's own value is its intact
    // tag bits.
    let bits: Vec<&str> = ir
        .lines()
        .filter_map(|line| {
            let (result, operand) = line.trim().split_once(" = bitcast double ")?;
            (operand == format!("{value} to i64")).then_some(result)
        })
        .collect();
    assert!(
        !bits.is_empty(),
        "getter result must be classified by its intact value tag"
    );
    // #9366's invariant, one indirection later: T1 moved the INT32 class-ref
    // arm (and its `js_typed_feedback_object_get_field_by_name_f64` call) into
    // the shared read exit (`js_object_get_field_value`), which routes on
    // the tag — so the bits it
    // receives must still be the getter's UNMASKED value. A masked handle here
    // would lose the 0x7FFE tag and the class would dispatch as an object.
    assert!(
        ir.lines().any(|line| {
            line.contains("call double @js_object_get_field_value_site(")
                && bits.iter().any(|b| line.contains(&format!("(i64 {b},")))
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
            byte_end: 0,
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

/// The generic property read's hit is ONE leaf call of the receiver layout's
/// probe (`perry_probe_get_memo`) over the agent's shape directory, the key,
/// the receiver's handle and the site's five integer shape/slot pairs. There
/// is no inline ShapeId compare, no way token and no epoch; the receiver's
/// own shape is the authority.
#[test]
fn generic_property_get_hit_path_is_shape_id_only() {
    let ir = emit(false, None);
    let blocks = tower_blocks(&ir);
    let (_, ok) = tower_block(&blocks, "pget.recv_ok");
    let probes: Vec<&String> = ok
        .iter()
        .filter(|l| l.contains("call double @perry_probe_get_memo("))
        .collect();
    assert_eq!(probes.len(), 1, "the hit is one probe call: {ok:?}");
    assert!(
        ok.iter().filter(|l| l.contains("call ")).count() == 1
            && !ok.iter().any(|l| l.contains("icmp eq i32")),
        "the hit block makes no other call and compares no site-cached \
         ShapeId: {ok:?}"
    );
    assert!(
        ir.contains(&format!(
            "{} = private global [5 x i64] zeroinitializer, align 8",
            front_contract::verify_front_site_memo(&blocks).unwrap()
        )),
        "the read must own exactly five zero-initialized shape/slot pairs:\n{ir}"
    );
    assert!(
        !ir.contains("4611686018427387904"),
        "no discriminated way token may be formed at the site:\n{ir}"
    );
    assert!(
        !ir.contains("@PERRY_IC_EPOCH"),
        "the removed pointer-token epoch must not appear in emitted IR:\n{ir}"
    );
}

/// An Array-annotated `.length` read inlines only the live plain Array (the
/// GC header's kind/forwarded halfword == ARRAY, then the u32 length word);
/// every other value, Array subclasses and byte views included, takes the one
/// runtime call that keeps full property semantics.
#[test]
fn guarded_length_read_emits_array_subclass_scalar_ic() {
    let ir = emit_guarded_length_read();
    for block in ["plen.check_gc", "plen.fast", "plen.slow", "plen.merge"] {
        assert!(
            ir.contains(block),
            "missing {block} from length read:\n{ir}"
        );
    }
    let fast = super::super::class_field_barrier_tests::block_body(&ir, "plen.fast.")
        .expect("the plain-array arm exists");
    assert!(
        fast.contains("load i32, ptr") && fast.contains("uitofp i32"),
        "the plain-array arm loads the u32 length word:\n{fast}"
    );
    assert!(
        !fast.contains("call "),
        "the plain-array arm must not call out:\n{fast}"
    );
    let slow = super::super::class_field_barrier_tests::block_body(&ir, "plen.slow.")
        .expect("the semantic arm exists");
    assert!(
        slow.contains("call double @js_value_length_property_f64("),
        "everything else must take the full-semantics length helper:\n{slow}"
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
            byte_end: 0,
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
            byte_end: 0,
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
        byte_end: 0,
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

/// The emitted function holding the generic tower, split into
/// `(label, trimmed body lines)` blocks. Register names restart in every
/// function, so every def/use question must be asked inside this one.
fn tower_blocks(ir: &str) -> Vec<(String, Vec<String>)> {
    let func = ir
        .split("\ndefine ")
        .find(|f| f.contains("\npget.slow"))
        .unwrap_or_else(|| panic!("no function contains the generic tower:\n{ir}"));
    let mut blocks: Vec<(String, Vec<String>)> = Vec::new();
    for line in func.lines() {
        if !line.starts_with(' ') && line.ends_with(':') {
            blocks.push((line.trim_end_matches(':').to_string(), Vec::new()));
        } else if let Some((_, body)) = blocks.last_mut() {
            if !line.trim().is_empty() {
                body.push(line.trim().to_string());
            }
        }
    }
    blocks
}

/// The one block whose label starts with `prefix`.
fn tower_block<'b>(blocks: &'b [(String, Vec<String>)], prefix: &str) -> (&'b str, &'b [String]) {
    let found: Vec<_> = blocks
        .iter()
        .filter(|(l, _)| l.starts_with(prefix))
        .collect();
    assert_eq!(found.len(), 1, "expected one `{prefix}` block: {blocks:?}");
    (found[0].0.as_str(), &found[0].1)
}

/// `(cond, true target, false target)` of a block's `br i1` terminator.
fn tower_cond_br(body: &[String]) -> (String, String, String) {
    let term = body.last().expect("a terminated block");
    let parts: Vec<&str> = term
        .strip_prefix("br i1 ")
        .unwrap_or_else(|| panic!("expected a conditional branch: {term}"))
        .split(", ")
        .collect();
    let label = |s: &str| s.trim_start_matches("label %").to_string();
    (parts[0].to_string(), label(parts[1]), label(parts[2]))
}

/// The probe is asked BEFORE anything that collects. The receiver-pointer
/// test's true edge is the probe's block; the probe's answer is compared with
/// the `TAG_HOLE` decline; the served edge is the merge and the declined edge
/// is the shared collecting exit, `js_object_get_field_value`. (This replaced
/// the polymorphic-ways ordering test: the ways are gone, the layout probe is the
/// whole of the first stage.)
#[test]
fn generic_property_get_tries_ways_before_calling_the_miss_handler() {
    let ir = emit(false, None);
    let blocks = tower_blocks(&ir);
    let (probe_label, probe_block) = front_call_block(&blocks);
    assert!(
        probe_block
            .iter()
            .any(|l| l.contains("call double @perry_probe_get_memo(")),
        "{probe_block:?}"
    );
    assert!(
        !probe_block
            .iter()
            .any(|l| l.contains("@js_object_get_field_value_site(")),
        "the slow call must not sit inside the probe block: {probe_block:?}"
    );
    let (_, served, declined) = tower_cond_br(probe_block);
    assert!(served.starts_with("pget.recv_merge"), "{probe_block:?}");
    assert!(declined.starts_with("pget.slow"), "{probe_block:?}");
    // The slow block joins the probe decline and the receiver-test refusal.
    let (slow_label, _) = tower_block(&blocks, "pget.slow");
    let preds: Vec<&str> = blocks
        .iter()
        .filter(|(_, body)| {
            body.iter()
                .any(|l| l.starts_with("br ") && l.contains(&format!("label %{slow_label}")))
        })
        .map(|(l, _)| l.as_str())
        .collect();
    assert_eq!(
        preds.len(),
        2,
        "receiver and probe refusals share the slow call"
    );
    assert!(preds.contains(&probe_label));
    assert!(preds.contains(&tower_block(&blocks, "pget.recv_other").0));
}

/// The shared refusal classifies the original boxed receiver in the runtime.
/// It reloads the key on the refusal edge, makes exactly one call and no
/// comparison, and the probe itself holds no per-site state.
#[test]
fn generic_refusal_keeps_boxed_receiver_and_reloads_key_on_the_join() {
    let ir = emit(false, None);
    let blocks = tower_blocks(&ir);
    let (_, slow) = tower_block(&blocks, "pget.slow");
    assert_eq!(slow.iter().filter(|l| l.contains("icmp ")).count(), 0);
    assert_eq!(slow.iter().filter(|l| l.contains(" call ")).count(), 1);
    let call = slow
        .iter()
        .find(|l| l.contains("@js_object_get_field_value_site("))
        .unwrap();
    let bits = call
        .split_once("(i64 ")
        .unwrap()
        .1
        .split(',')
        .next()
        .unwrap();
    assert!(
        blocks.iter().flat_map(|(_, body)| body).any(|line| line
            .starts_with(&format!("{bits} = bitcast double "))
            && line.ends_with(" to i64")),
        "original receiver bits: {call}"
    );
    assert!(
        slow.iter().any(|l| l.contains("load double, ptr @")),
        "fallback key is reloaded at the refusal join: {slow:?}"
    );
    assert!(
        !slow.iter().any(|l| l.contains("add i64")),
        "the shared call cannot use a pointer-only handle: {slow:?}"
    );
    let all: Vec<&String> = blocks.iter().flat_map(|(_, b)| b.iter()).collect();
    assert!(
        !all.iter().any(|l| l.contains("@PERRY_IC_EPOCH")),
        "the removed keys-pointer epoch global must not appear"
    );
    assert!(
        front_contract::verify_front_site_memo(&blocks).is_ok(),
        "the refusal must pass the leaf's own shape/slot storage"
    );
}

/// A SPILL-located key is served by the runtime probe, not by site code: the
/// site expands no spill recognition (no flipped-ShapeId constant, no spill
/// load arithmetic), and the probe's answer reaches the merge straight from
/// its own block.
#[test]
fn a_spill_entry_is_served_by_the_leaf_front_before_the_slow_call() {
    use crate::expr::property_get::generic_dispatch::PACKED_SPILL_FLIP;
    let ir = emit(false, None);
    let blocks = tower_blocks(&ir);
    let (probe_label, probe_block) = front_call_block(&blocks);
    let call = probe_block
        .iter()
        .find(|l| l.contains("@perry_probe_get_memo("))
        .unwrap_or_else(|| panic!("the probe call: {probe_block:?}"));
    let answer = call.split_once(" = ").map(|(r, _)| r).unwrap();
    let (_, merge) = tower_block(&blocks, "pget.recv_merge");
    let phi = merge.iter().find(|l| l.contains(" = phi double ")).unwrap();
    assert!(
        phi.contains(&format!("[ {answer}, %{probe_label} ]")),
        "the merge must take the probe's answer straight from its block: {phi}"
    );
    for (label, body) in &blocks {
        for gone in [
            PACKED_SPILL_FLIP.to_string(),
            "pic.spill".to_string(),
            "xor i32 ".to_string(),
        ] {
            assert!(
                !label.starts_with(gone.as_str()) && !body.iter().any(|l| l.contains(&gone)),
                "no spill recognition may be expanded at the site, found \
                 `{gone}` in {label}: {body:?}"
            );
        }
    }
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
            byte_end: 0,
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
            byte_end: 0,
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

/// The object-backed (Array-subclass) `.length` is not expanded inline: the
/// elements store, the shape and the family are the runtime helper's to read,
/// so the site holds no `ObjectMeta` probe of its own.
#[test]
fn the_length_tier_probes_the_elements_store_before_the_shape_ic() {
    let ir = emit_guarded_length_read();
    assert!(
        !ir.contains("plen.elem") && !ir.contains("plen.ic."),
        "the elements/shape tiers live in the runtime helper:\n{ir}"
    );
    let slow = super::super::class_field_barrier_tests::block_body(&ir, "plen.slow.")
        .expect("the semantic arm exists");
    assert_eq!(
        slow.matches(" call ").count(),
        1,
        "a non-plain-array receiver is one helper call:\n{slow}"
    );
}

/// First-read D3: the miss front's directory operand (`PERRY_AGENT_PTRS`
/// slot 0) is read WITHOUT a call wherever the target has a call-free
/// thread-pointer path: an initial-exec load on an ELF executable, the TEB's
/// TLS array on Windows x86-64 (`agent_ptr::AgentPtrAccess::WindowsTeb`), the
/// pthread TSD on Apple aarch64. x86-64 Darwin keeps the `gc-leaf` accessor:
/// Mach-O has no call-free thread-local model there (`agent_ptr.rs`).
#[test]
fn the_front_reads_its_directory_without_a_call_where_the_target_allows() {
    for (target, inline_form) in [
        (
            "x86_64-unknown-linux-gnu",
            Some("getelementptr i8, ptr @PERRY_AGENT_PTRS, i64 0"),
        ),
        (
            "x86_64-pc-windows-msvc",
            Some("load ptr, ptr addrspace(256) inttoptr (i64 88 to ptr addrspace(256))"),
        ),
        ("aarch64-apple-darwin", Some("mrs $0, tpidrro_el0")),
        ("x86_64-apple-darwin", None),
    ] {
        let mut opts = ir_opts(false, None);
        opts.target = Some(target.to_string());
        let ir =
            String::from_utf8(compile_module(&module_with_nullish_read(), opts).unwrap()).unwrap();
        let func = ir
            .split("\ndefine ")
            .find(|f| f.contains("\npget.slow"))
            .unwrap_or_else(|| panic!("{target}: no function contains the probe:\n{ir}"));
        let blocks = tower_blocks(&ir);
        front_call_block(&blocks);
        verify_front_directory(&blocks).unwrap_or_else(|e| panic!("{target}: {e}\n{func}"));
        let dir_call = func.contains("call ptr @perry_shape_dir_cell(");
        match inline_form {
            Some(form) => {
                assert!(
                    func.contains(form) && !dir_call,
                    "{target}: the directory must be read inline (`{form}`), \
                     with no accessor call:\n{func}"
                );
            }
            None => assert!(dir_call, "{target}: the accessor call:\n{func}"),
        }
        if target.contains("windows") {
            for global in ["@_tls_index", "@PERRY_AGENT_PTRS_SECREL"] {
                assert!(
                    func.contains(&format!("load i32, ptr {global}")),
                    "{target}: the TEB form reads {global}:\n{func}"
                );
            }
        }
    }
}

/// Per untyped `obj.prop` the emitted read is a bounded handful of blocks and
/// calls, with only the probe inline.
///
/// This is a ratchet, so it is an EXACT count in both dimensions: the probe
/// (a GC-leaf, nounwind call) and one collecting exit shared by a probe
/// decline and a receiver that is not a heap pointer. Each extra property-GET
/// call site is a statepoint whose live GC values are written into `.perry_gcmap`; one creeping back inline is a
/// regression measured in `.text` that nothing else in the suite reports.
#[test]
fn the_generic_tower_is_one_leaf_call_one_exit_and_a_bounded_number_of_blocks() {
    let ir = emit(false, None);
    let func = ir
        .split("\ndefine ")
        .find(|f| f.contains("\npget.slow"))
        .unwrap_or_else(|| panic!("no function contains the generic tower:\n{ir}"));

    let pget_calls: Vec<&str> = func
        .lines()
        .filter(|l| l.contains(" call ") || l.contains(" invoke "))
        .filter_map(|l| l.split(" @").nth(1))
        .filter_map(|c| c.split('(').next())
        .filter(|c| {
            c.starts_with("js_object_get_field")
                || c.starts_with("js_typed_feedback_object_get_field")
                || c.starts_with("perry_probe_get")
                || *c == "js_throw_type_error_property_access"
        })
        .collect();
    let mut sorted = pget_calls.clone();
    sorted.sort();
    assert_eq!(
        sorted,
        vec!["js_object_get_field_value_site", "perry_probe_get_memo"],
        "the read must expand the probe and exactly one collecting exit:\n{func}"
    );
    // The probe is nounwind and GC-leaf: a plain call, never an invoke.
    let probes: Vec<&str> = func
        .lines()
        .filter(|l| l.contains("@perry_probe_get_memo("))
        .collect();
    assert_eq!(probes.len(), 1, "one probe call per read:\n{func}");
    assert!(
        probes[0].contains(" = call double "),
        "the probe is nounwind, a plain call:\n{func}"
    );
    // A non-`length` site reads this agent's own directory, never the empty one.
    assert!(
        !probes[0].contains("@PERRY_EMPTY_SHAPE_DIR"),
        "only an unreadable directory passes the empty directory:\n{}",
        probes[0]
    );
    verify_front_directory(&tower_blocks(&ir)).unwrap_or_else(|e| panic!("{e}\n{func}"));

    let blocks: Vec<&str> = func
        .lines()
        .filter(|l| !l.starts_with(' ') && l.ends_with(':'))
        .map(|l| l.trim_end_matches(':'))
        .filter(|l| l.starts_with("pget.") || l.starts_with("pic."))
        .collect();
    let mut expected = vec![
        // the receiver test and the inline probe
        "pget.recv_ok",
        // the tag test's refusal edge joins the collecting exit
        "pget.recv_other",
        // the shared collecting refusal
        "pget.slow",
        // the join
        "pget.recv_merge",
    ];
    // Labels carry a numeric suffix (`pget.slow.6`); strip it for comparison.
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
        "the emitted read's block set changed:\n{func}"
    );
}

/// The generic read's collecting entry is shared by both refusals. It takes
/// the boxed receiver, key and feedback-site ID; the merge uses its answer.
#[test]
fn the_generic_shared_read_receives_boxed_receiver_key_and_feedback() {
    let ir = emit(false, None);
    let blocks = tower_blocks(&ir);
    let slow_callers: Vec<(&str, &String)> = blocks
        .iter()
        .flat_map(|(l, body)| {
            body.iter()
                .filter(|t| t.contains("@js_object_get_field_value_site("))
                .map(move |t| (l.as_str(), t))
        })
        .collect();
    assert_eq!(slow_callers.len(), 1, "{slow_callers:?}");
    let (call_label, slow_line) = slow_callers[0];
    assert!(
        call_label.starts_with("pget.slow"),
        "the slow entry must be called from the one exit: {slow_callers:?}"
    );
    assert!(
        slow_line.contains("(i64 %") && slow_line.matches(", i64 ").count() == 2,
        "the shared entry must receive boxed receiver, key and feedback ID:\n{slow_line}"
    );
    let (_, probe_block) = front_call_block(&blocks);
    let (cond, served, declined) = tower_cond_br(probe_block);
    assert!(
        probe_block
            .iter()
            .any(|l| l.starts_with(&format!("{cond} = icmp ne i64 "))
                && l.ends_with(crate::nanbox::TAG_HOLE_I64)),
        "the probe's branch must be on its TAG_HOLE decline: {probe_block:?}"
    );
    assert!(served.starts_with("pget.recv_merge"), "{probe_block:?}");
    assert_eq!(declined, call_label, "{probe_block:?}");
    let (_, merge) = tower_block(&blocks, "pget.recv_merge");
    let phi = merge.iter().find(|l| l.contains(" = phi double ")).unwrap();
    let value = slow_line.split_once(" = ").map(|(v, _)| v).unwrap();
    assert!(
        phi.contains(&format!("[ {value}, %{call_label} ]")),
        "the merge must take the slow entry's value from `{call_label}`:\n{phi}"
    );
}

#[path = "array_length_tests.rs"]
mod array_length;

#[test]
fn guarded_length_reads_admit_byte_views_and_use_a_pooled_cold_key() {
    let ir = emit_guarded_length_read();
    // Byte views and metadata overrides are answered by the helper, so the
    // guarded read inlines no typed-array arm that an override could stale.
    assert!(
        !ir.contains("plen.typed_array") && !ir.contains("plen.byte_header"),
        "no inline typed-array arm on the guarded read:\n{ir}"
    );
    assert!(
        ir.contains("call double @js_value_length_property_f64(double"),
        "byte views take the semantic helper:\n{ir}"
    );
}

#[path = "computed_admission_tests.rs"]
mod computed_admission;

#[path = "refusal_join_tests.rs"]
mod refusal_join;
