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
    assert_eq!(
        PIC_WAY_BASE + PIC_WAYS * 2,
        PIC_CACHE_WORDS,
        "the ways must fill the emitted global exactly"
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
        !ways_body.contains("call double @js_object_get_field_ic_miss"),
        "the miss call must not sit inside the way block:\n{ways_body}"
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
/// which left `token` / `token_nonnull` / `epoch_eq` live on only some of them
/// and forced the block to reload the whole header ladder. That block is not
/// cold: on a receiver rotation wider than the MRU entry it runs on nearly
/// every read, so the duplicate ladder was hot code. The fix is purely
/// structural — send the two receiver-validation failures to `pic.miss.cold`
/// (they can never resolve a way, since `way_hit` requires a real object) and
/// the dominance follows.
///
/// Assert the *consequences*, not the block names alone: a re-derivation would
/// show up as a second `@PERRY_IC_EPOCH` load and as the small-handle sentinel
/// `select`, and both must be gone.
#[test]
fn pic_miss_reuses_the_token_blocks_values_instead_of_re_deriving_them() {
    let ir = emit(false, None);
    assert!(
        ir.contains("@perry_ic_"),
        "test premise: the generic read reaches the inline PIC:\n{ir}"
    );
    assert!(
        ir.contains("\npic.miss.cold"),
        "the two receiver-validation failures need their own landing block, \
         otherwise pic.miss is not dominated by pic.token:\n{ir}"
    );
    let epoch_loads = ir.matches("load i64, ptr @PERRY_IC_EPOCH").count();
    assert_eq!(
        epoch_loads, 1,
        "one generic read must load @PERRY_IC_EPOCH exactly once; a second \
         load means the way block re-derived the epoch predicate:\n{ir}"
    );
    assert!(
        !ir.contains("ptrtoint ptr @perry_ic_"),
        "the small-handle sentinel select only existed because an invalid \
         receiver could reach the way compares; it must be gone:\n{ir}"
    );
    // The header predicates: each load/compare pair must appear exactly once.
    for (needle, what) in [
        ("icmp eq i8 ", "the GC_TYPE_OBJECT compare"),
        ("icmp eq i32 %", "the closure-magic / object_type compares"),
    ] {
        let n = ir.matches(needle).count();
        assert!(
            n <= 2,
            "{what} appears {n} times — the miss block is re-deriving the \
             receiver header again:\n{ir}"
        );
    }
}

/// #7907: the cached-slot bound is `slot < FLOOR || slot < field_count`, not
/// `slot < max(field_count, FLOOR)`.
///
/// Identical predicate; the point is that the `max` had to be materialised, and
/// the `csel` that did it sat on the dependency chain out of the `field_count`
/// load — the single hottest instruction in `interp.ts`'s `evalNode`. If
/// someone "simplifies" this back to a `max`, nothing else in the suite
/// notices.
#[test]
fn cached_slot_bound_is_a_disjunction_not_a_materialised_max() {
    let ir = emit(false, None);
    assert!(
        ir.contains("icmp ult i64 ") && ir.contains(", 4"),
        "test premise: the emitted bound compares a slot against \
         INLINE_SLOT_FLOOR:\n{ir}"
    );
    assert!(
        !ir.contains(", i64 4, i64 %"),
        "a `select …, i64 4, i64 %fc` is the materialised max this deliberately \
         does not emit:\n{ir}"
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
    let ir = emit(false, None);

    // Register names restart at %r1 in every function, so the walk MUST be
    // scoped to one function or the def map silently resolves a condition to
    // an identically-named register in a different body (this test read a
    // string-handle `ptrtoint` as the receiver-tag test before it was fixed).
    let func = ir
        .split("\ndefine ")
        .find(|f| f.contains("pic.hit.load"))
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
        .find(|(l, _)| l.starts_with("pic.hit.load"))
        .map(|(l, _)| l.clone())
        .unwrap_or_else(|| panic!("no `pic.hit.load` block:\n{func}"));

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
        conds.len() >= 5,
        "expected at least five guard branches between the PIC entry and the \
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
    for (needle, what) in [
        ("32765", "the POINTER/STRING receiver-tag test"),
        ("1048575", "the small-handle (native registry id) test"),
        ("icmp eq i8", "the GcHeader obj_type == GC_TYPE_OBJECT test"),
        ("1129268819", "the CLOSURE_MAGIC test"),
        ("2048", "the OBJ_FLAG_HAS_DESCRIPTORS test"),
        ("@PERRY_IC_EPOCH", "the read-PIC epoch gate"),
        ("@perry_ic_", "the per-site cached shape-token compare"),
    ] {
        assert!(
            chain.contains(needle),
            "the inline slot load must be gated on {what}, but no branch \
             condition on the path to `{load_label}` depends on it.\n\
             conditions: {conds:?}\nreached def chain:\n{chain}\n\nIR:\n{func}"
        );
    }
}
