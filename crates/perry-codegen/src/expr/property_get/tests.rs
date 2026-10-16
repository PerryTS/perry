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
use front_contract::{front_call_block, verify_accessor_arm, verify_front_directory};

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
///
/// First-read D3: the hit is the receiver's `+4` word compared, as an `i32`,
/// with the compact word's low half — no discriminated token is formed at the
/// site at all. The polymorphic ways' `PIC_ID_TOKEN_BIT | ShapeId` tokens are
/// compared inside the miss front (`js_object_get_field_ic_front`), so the
/// token bit appearing in emitted IR again would mean a way compare crept back
/// inline.
#[test]
fn generic_property_get_hit_path_is_shape_id_only() {
    let ir = emit(false, None);
    assert!(
        ir.contains("@perry_ic_"),
        "test premise: the generic read reaches the inline monomorphic PIC:\n{ir}"
    );
    let token = ir
        .find("\npic.token")
        .unwrap_or_else(|| panic!("expected a pic.token block:\n{ir}"));
    let token_body = &ir[token
        ..ir[token + 1..]
            .find("\n\n")
            .map(|o| o + token + 1)
            .unwrap_or(ir.len())];
    assert!(
        token_body.contains("load i32")
            && token_body.contains("trunc i64")
            && token_body.contains("icmp eq i32"),
        "the hit is the ShapeId word compared with the compact word's low \
         half:\n{token_body}"
    );
    assert!(
        !ir.contains("4611686018427387904"),
        "no discriminated way token may be formed at the site — the ways are \
         the miss front's:\n{ir}"
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
        ir.contains("call double @js_value_length_property_key_ic_f64"),
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
        ir.contains("call double @js_value_length_property_key_ic_f64"),
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
            && ir.contains("call double @js_value_length_property_key_ic_f64"),
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

/// The emitted function holding the generic tower, split into
/// `(label, trimmed body lines)` blocks. Register names restart in every
/// function, so every def/use question must be asked inside this one.
fn tower_blocks(ir: &str) -> Vec<(String, Vec<String>)> {
    let func = ir
        .split("\ndefine ")
        .find(|f| f.contains("\npic.miss.call"))
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

/// #7753: the polymorphic ways must be consulted BEFORE the collecting miss
/// call, and the monomorphic path must not have grown any work.
///
/// A one-entry cache misses on essentially every read at a site whose receiver
/// alternates between shapes — the shape of every discriminated-union dispatch
/// — and each miss that reaches the collecting slow entry pays its statepoint
/// and the full miss ladder. If the ways are ever moved behind that call they
/// stop paying for themselves, and nothing else in the suite would show it —
/// the program still computes the right answer, just slowly. So assert the
/// ORDER, not merely the presence.
///
/// First-read D3: the ways are asked by the GC-leaf miss front
/// (`js_object_get_field_ic_front`, ways first — pinned by
/// `read_confirm::tests::the_front_answers_a_way_and_declines_a_null_cache`),
/// so the order is a CFG fact here: the ShapeId compare's false edge is the
/// front, the front's SERVED edge is the merge, and the slow call is reached
/// from the front only on its decline edge.
#[test]
fn generic_property_get_tries_ways_before_calling_the_miss_handler() {
    let ir = emit(false, None);
    let blocks = tower_blocks(&ir);
    let (_, token) = tower_block(&blocks, "pic.token");
    let (_, on_hit, on_miss) = tower_cond_br(token);
    assert!(on_hit.starts_with("pic.hit"), "{token:?}");
    // #10498: the class-accessor arm sits on the miss edge; every one of its
    // guards declines to the front, so the front (the ways) is still asked
    // before anything that collects except a proven accessor hit.
    let arm = verify_accessor_arm(&blocks).unwrap_or_else(|e| panic!("{e}: {blocks:?}"));
    assert!(
        on_miss == arm[0],
        "the compare's miss edge must reach the accessor arm, then the front          (the ways): {token:?}"
    );
    let (front_label, front) = front_call_block(&blocks);
    assert!(
        front
            .iter()
            .any(|l| l.contains("call double @js_object_get_field_ic_front(")),
        "{front:?}"
    );
    assert!(
        !front
            .iter()
            .any(|l| l.contains("@js_object_get_field_ic_slow(")),
        "the slow call must not sit inside the front block: {front:?}"
    );
    let (_, served, declined) = tower_cond_br(front);
    assert!(served.starts_with("pget.recv_merge"), "{front:?}");
    assert!(declined.starts_with("pic.miss.call"), "{front:?}");
    // Every way of reaching the slow call from the object path goes through
    // the front: its only other predecessor is the receiver-validation
    // failure, which the front could not answer (no real object).
    let (call_label, _) = tower_block(&blocks, "pic.miss.call");
    let preds: Vec<&str> = blocks
        .iter()
        .filter(|(_, body)| {
            body.iter()
                .any(|l| l.starts_with("br ") && l.contains(&format!("label %{call_label}")))
        })
        .map(|(l, _)| l.as_str())
        .collect();
    assert!(
        preds.contains(&front_label)
            && preds.iter().all(|p| *p == front_label
                || p.starts_with("pget.recv_")
                || p.starts_with("pic.acc.kind.")),
        "the slow call is reached from the front's decline or a receiver \
         failure only: {preds:?}"
    );
}

/// #7907: the miss path must be DOMINATED by `pic.token`, so it can use the
/// values that block already computed instead of re-deriving them.
///
/// #7883 routed all four failure edges — small-handle receiver, non-object
/// receiver, MRU token mismatch, cached slot out of bounds — into one block,
/// which left the token values live on only some of them and forced the block
/// to reload the whole header ladder. That block is not cold: on a receiver
/// rotation wider than the MRU entry it runs on nearly every read, so the
/// duplicate ladder was hot code. The receiver-validation failures go to the
/// slow exit directly (they can never resolve a way: a way hit requires a real
/// object), and the dominance follows.
///
/// First-read D3: that block is `pic.miss.front`, the GC-leaf front call. It
/// must have exactly ONE predecessor, `pic.token`'s false edge, and the site
/// must not re-derive a receiver predicate for it: the front re-reads the
/// ShapeId word itself, so the hot load keeps a single use (the compare) and
/// isel folds it into `cmp %ecx, 4(%rdi)`.
#[test]
fn pic_miss_reuses_the_token_blocks_values_instead_of_re_deriving_them() {
    let ir = emit(false, None);
    let blocks = tower_blocks(&ir);
    let (front_label, _) = tower_block(&blocks, "pic.miss.front");
    let (_, token) = tower_block(&blocks, "pic.token");
    let preds: Vec<&str> = blocks
        .iter()
        .filter(|(_, body)| {
            body.iter().any(|l| {
                l.starts_with("br ")
                    && l.split("label %")
                        .skip(1)
                        .any(|t| t.trim_end_matches(&[',', ' '][..]) == front_label)
            })
        })
        .map(|(l, _)| l.as_str())
        .collect();
    // #10498: the front's predecessors are the class-accessor arm's guards,
    // a chain the token compare's false edge enters and dominates.
    let arm = verify_accessor_arm(&blocks).unwrap_or_else(|e| panic!("{e}: {blocks:?}"));
    assert_eq!(
        tower_cond_br(token).2,
        arm[0],
        "the token compare's false edge must enter the arm: {token:?}"
    );
    assert_eq!(
        preds, arm,
        "the front must be reached only through the accessor arm's guards, \
         or it is no longer dominated by pic.token: {blocks:?}"
    );
    let all: Vec<&String> = blocks.iter().flat_map(|(_, b)| b.iter()).collect();
    // The arm re-reads the receiver's ShapeId on the miss edge on purpose
    // (pinned by `verify_accessor_arm`); the predicate counts below are about
    // the token path.
    let token_path: Vec<&String> = blocks
        .iter()
        .filter(|(l, _)| !l.starts_with("pic.acc."))
        .flat_map(|(_, b)| b.iter())
        .collect();
    assert!(
        !all.iter().any(|l| l.contains("@PERRY_IC_EPOCH")),
        "the removed keys-pointer epoch global must not appear"
    );
    assert!(
        !all.iter().any(|l| l.contains("ptrtoint ptr @perry_ic_")),
        "the small-handle sentinel select must be gone"
    );
    // The receiver predicates, exactly once each: the ShapeId identity
    // compare is the only `icmp eq i32`, and there is no GC-kind compare at
    // all (#10828).
    for (needle, what, expect) in [
        ("icmp eq i8 ", "the GC_TYPE_OBJECT compare", 0),
        ("icmp eq i32 %", "the ShapeId identity compare", 1),
    ] {
        let n = token_path.iter().filter(|l| l.contains(needle)).count();
        assert_eq!(
            n, expect,
            "{what} appears {n} times, expected {expect} — a receiver \
             predicate is being re-derived or has crept back: {blocks:?}"
        );
    }
    let shape_word = token
        .iter()
        .find(|l| l.contains(" = load i32, "))
        .and_then(|l| l.split_once(" = "))
        .map(|(r, _)| r.to_string())
        .unwrap_or_else(|| panic!("the ShapeId word load: {token:?}"));
    let uses = all
        .iter()
        .filter(|l| {
            l.split(|c: char| c == ',' || c == ' ' || c == '(' || c == ')')
                .any(|t| t == shape_word)
        })
        .count();
    assert_eq!(
        uses, 2,
        "the hot ShapeId load must have exactly one use (the compare), so it \
         folds into it and stays dead on the miss edge: {blocks:?}"
    );
}

/// S5: a matched SPILL entry is served without reaching the collecting call.
///
/// First-read D3: the miss front recognises it — the compact word holding the
/// receiver's ShapeId flipped by `PACKED_SPILL_FLIP` — and answers with the
/// three dependent loads the ShapeId licenses (`ObjectHeader.meta`,
/// `ObjectMeta.spill`, the element at the word's index); its behaviour is
/// `read_confirm::tests::the_front_serves_a_spill_entry_only_for_a_real_shape_id`.
/// The CODEGEN half: the site hands the front the compact word (the spill
/// index and flipped id live there), the front's SERVED edge lands on the
/// merge without a second call, and no spill arithmetic is expanded inline.
#[test]
fn a_spill_entry_is_served_by_the_leaf_front_before_the_slow_call() {
    use crate::expr::property_get::generic_dispatch::PACKED_SPILL_FLIP;
    let ir = emit(false, None);
    let blocks = tower_blocks(&ir);
    let (front_label, front) = front_call_block(&blocks);
    let call = front
        .iter()
        .find(|l| l.contains("@js_object_get_field_ic_front("))
        .unwrap_or_else(|| panic!("the front call: {front:?}"));
    assert!(
        call.ends_with("_packed_get)"),
        "the front must receive the compact word it decodes a spill entry \
         from: {call}"
    );
    let answer = call.split_once(" = ").map(|(r, _)| r).unwrap();
    let (_, merge) = tower_block(&blocks, "pget.recv_merge");
    let phi = merge.iter().find(|l| l.contains(" = phi double ")).unwrap();
    assert!(
        phi.contains(&format!("[ {answer}, %{front_label} ]")),
        "the merge must take the front's answer straight from its block: {phi}"
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
            .find(|f| f.contains("\npic.miss.front"))
            .unwrap_or_else(|| panic!("{target}: no function contains the front:\n{ir}"));
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

/// T1: per untyped `obj.prop` the emitted tower is a bounded handful of blocks
/// and calls, with only the inline hit kept inline.
///
/// This is a ratchet, so it is an EXACT count in both dimensions. The tower T1
/// replaced expanded 33 tower blocks and SIX runtime call sites per site
/// (`js_object_get_field_by_name_f64` twice, the feedback-wrapped class-ref
/// helper, `js_throw_type_error_property_access`,
/// `js_object_get_field_ic_overflow_load`, `js_object_get_field_ic_miss_packed`),
/// each one a statepoint whose live GC values are written into
/// `.perry_gcmap`. On @babel/parser that was 29% of all emitted IR over 6,487
/// sites; a single arm creeping back inline is a regression measured in
/// megabytes of `.text`, and nothing else in the suite would report it.
///
/// Two collecting exits and not one: a single shared exit let SimplifyCFG fold
/// the receiver-tag test and the small-handle test into one flat predicate,
/// costing +4.00 instructions on every HIT (measured, before those two tests
/// became the one fused compare). The separate non-pointer callee still keeps
/// the `.length` tower's chain branchy — a change that makes it one is a
/// hit-path regression, not a size win.
///
/// First-read D3: the spill entry, the ways, the inherited-read cache and the
/// latched confirm are no longer expanded per site. The ShapeId compare's
/// false edge makes ONE plain call to the GC-leaf front
/// (`js_object_get_field_ic_front`), whose `TAG_HOLE` decline continues to the
/// collecting slow call.
///
/// #10498: ahead of the front, the class-accessor arm may make one more call,
/// an indirect call of a compiled getter it has proved (`verify_accessor_arm`);
/// it calls no runtime property entry, so the property-GET family below is
/// unchanged.
#[test]
fn the_generic_tower_is_one_leaf_call_two_exits_and_a_bounded_number_of_blocks() {
    let ir = emit(false, None);
    let func = ir
        .split("\ndefine ")
        .find(|f| f.contains("\npic.miss.call"))
        .unwrap_or_else(|| panic!("no function contains the generic tower:\n{ir}"));

    // Every call/invoke in the whole function, by callee. Feedback records are
    // compile-time gated and absent from this build; anything else must be the
    // front or one of the two exits (the fixture's module init contributes its
    // own calls, so match on the property-GET family rather than on a total).
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
            "js_object_get_field_ic_front",
            "js_object_get_field_ic_nonptr",
            "js_object_get_field_ic_slow"
        ],
        "the tower must expand the front and exactly two collecting exits:\n{func}"
    );
    // The front is nounwind: a plain call, never an invoke (its GC-leaf
    // classification is `gc_call_effects`' test).
    let fronts: Vec<&str> = func
        .lines()
        .filter(|l| l.contains("@js_object_get_field_ic_front("))
        .collect();
    assert_eq!(fronts.len(), 1, "one front call per tower:\n{func}");
    assert!(
        fronts[0].contains(" = call double "),
        "the front is nounwind, a plain call:\n{func}"
    );
    // A non-`length` site confirms from this agent's own directory: slot 0
    // of the target's per-agent block, or the empty directory when Apple's
    // direct TLS lookup is unavailable. Follow the actual call operand.
    assert!(
        !fronts[0].contains("@PERRY_EMPTY_SHAPE_DIR"),
        "only a `length` site passes the empty directory:\n{}",
        fronts[0]
    );
    verify_front_directory(&tower_blocks(&ir)).unwrap_or_else(|e| panic!("{e}\n{func}"));

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
        // its split (cold): a POINTER-tagged small handle fails the fused
        // receiver test too and goes on to the object exit from here
        "pget.recv_nonptr",
        // `pic.recv_hdr` is GONE: the ShapeId compare in `pic.token` proves
        // the kind (#10828) and the descriptor state (#10824).
        "pic.token",
        // The hit block is the slot load and a branch to the merge: no
        // overflow-bit test (a spill entry is refused by the compare itself)
        // and no `TAG_HOLE` compare (#10826 made delete a shape transition).
        "pic.hit",
        // First-read D3: the compare's false edge. One GC-leaf call answers
        // a way, a spill entry or a latched site's confirmed guess; its
        // decline continues to the one exit. `pic.token.miss`,
        // `pic.spill.hit`, `pic.token.ways`, `pic.miss`, `pic.ways`,
        // `pic.way.load`, `pic.not_ways`, `pic.mega` and `pic.miss.inherited`
        // are GONE into it and into the slow entry.
        "pic.miss.front",
        // the one exit, and the join
        "pic.miss.call",
        "pget.recv_merge",
        // #10498: the class-accessor arm on the compare's false edge, ahead of
        // the front: the shared guard program declines to the front, and the direct
        // getter call (`verify_accessor_arm` pins the chain).
        "pic.acc.empty",
        "pic.acc.kind",
    ];
    // Labels carry a numeric suffix (`pic.token.6`); strip it for comparison.
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

/// The generic read's collecting slow entry belongs on the miss-front decline.
/// Own-word and holder-shape hits bypass it. The call keeps all four operands,
/// and the merge uses the value returned by that one miss entry.
#[test]
fn the_generic_slow_read_is_called_only_after_the_front_declines() {
    let ir = emit(false, None);
    let blocks = tower_blocks(&ir);
    // 2.
    let slow_callers: Vec<(&str, &String)> = blocks
        .iter()
        .flat_map(|(l, body)| {
            body.iter()
                .filter(|t| t.contains("@js_object_get_field_ic_slow("))
                .map(move |t| (l.as_str(), t))
        })
        .collect();
    assert_eq!(slow_callers.len(), 1, "{slow_callers:?}");
    let (call_label, slow_line) = slow_callers[0];
    assert!(
        call_label.starts_with("pic.miss.call"),
        "the slow entry must be called from the one exit: {slow_callers:?}"
    );
    assert!(
        slow_line.contains("(i64 %")
            && slow_line.matches(", i64 %").count() == 1
            && slow_line.contains("ptr @perry_ic_")
            && slow_line.contains("_packed_get"),
        "the slow entry must still receive the receiver, the key, the cache \
         slot and the packed word:\n{slow_line}"
    );
    // 3.
    let (_, front) = front_call_block(&blocks);
    let (cond, served, declined) = tower_cond_br(front);
    assert!(
        front
            .iter()
            .any(|l| l.starts_with(&format!("{cond} = icmp ne i64 "))
                && l.ends_with(crate::nanbox::TAG_HOLE_I64)),
        "the front's branch must be on its TAG_HOLE decline: {front:?}"
    );
    assert!(served.starts_with("pget.recv_merge"), "{front:?}");
    assert_eq!(declined, call_label, "{front:?}");
    // 4.
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
    let typed = ir
        .split("\nplen.typed_array")
        .nth(1)
        .unwrap_or_else(|| panic!("expected the typed metadata guard:\n{ir}"));
    let typed = typed.split("\n\n").next().unwrap();
    assert!(
        typed.contains(", 64") && typed.contains(", 76"),
        "Buffer and Uint8Array type bytes must be admitted:\n{typed}"
    );
    assert!(
        ir.contains("@PERRY_TYPED_NAMED_PROPS_INVALIDATED") && ir.contains("plen.byte_header"),
        "metadata overrides must withdraw the proof:\n{typed}"
    );
    assert!(!ir.contains("call double @js_value_length_property_ic_f64"));
    assert!(ir.contains("call double @js_value_length_property_key_ic_f64"));
}

#[path = "global_read_tests.rs"]
mod global_read;

#[path = "computed_admission_tests.rs"]
mod computed_admission;
