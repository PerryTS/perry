/// #10399: a `thread_local` global's cross-unit DECLARATION must keep the
/// TLS specifier. Without it `external_decl_for_global` returned `None`
/// and `split_units` panicked with "cannot form external declaration"; a
/// declaration that dropped `thread_local` would name a different symbol
/// than the definition.
#[test]
fn external_decl_keeps_thread_local() {
    use crate::module::linkage::external_decl_for_global;
    assert_eq!(
        external_decl_for_global("@perry_global_m__0 = thread_local global double 0.0").as_deref(),
        Some("@perry_global_m__0 = external thread_local global double")
    );
    assert_eq!(
        external_decl_for_global("@__perry_init_done_m = internal thread_local global i8 0")
            .as_deref(),
        Some("@__perry_init_done_m = external thread_local global i8")
    );
    // The non-TLS form is unchanged.
    assert_eq!(
        external_decl_for_global("@perry_global_m__0 = global double 0.0").as_deref(),
        Some("@perry_global_m__0 = external global double")
    );
    // Aggregate types still parse with a TLS specifier in front.
    assert_eq!(
        external_decl_for_global(
            "@perry_class_header_m__C = internal thread_local global <2 x i64> zeroinitializer"
        )
        .as_deref(),
        Some("@perry_class_header_m__C = external thread_local global <2 x i64>")
    );
}

/// #10399: duplicating a TLS global across codegen units must not drop the
/// specifier either.
#[test]
fn unit_promotion_keeps_thread_local() {
    use crate::module::linkage::{make_unique_owner_global, promote_global_for_units};
    assert_eq!(
        promote_global_for_units("@g = internal thread_local global i8 0"),
        "@g = linkonce_odr thread_local global i8 0"
    );
    assert_eq!(
        make_unique_owner_global("@g = internal thread_local global i8 0"),
        "@g = thread_local global i8 0"
    );
}
use super::*;
use crate::types::{DOUBLE, I32, I64, PTR, VOID};

#[test]
fn owned_codegen_units_move_each_function_exactly_once() {
    let mut module = LlModule::new("x86_64-pc-windows-msvc");
    for name in ["first", "second", "third"] {
        let function = module.define_function(name, VOID, vec![]);
        function.create_block("entry").ret_void();
    }

    let units = module.into_codegen_unit_parts(2);
    assert_eq!(units.len(), 2);
    let mut names: Vec<String> = units
        .iter()
        .flat_map(|unit| unit.funcs.iter().map(|function| function.name.clone()))
        .collect();
    names.sort();
    assert_eq!(names, ["first", "second", "third"]);
}

#[test]
fn render_codegen_units_partitions_and_links() {
    // #5391: a 2-unit split of a 2-function module must (a) define each
    // function in exactly one unit, (b) declare the other so cross-unit
    // calls resolve, and (c) carry the shared globals in BOTH units with
    // local linkage promoted to linkonce_odr (linker dedups).
    let mut m = LlModule::new("x86_64-pc-windows-msvc");
    m.declare_function("js_console_log_number", VOID, &[DOUBLE]);
    m.add_internal_global("perry_global_x", DOUBLE, "0.0");
    let (_s, _l) = m.add_string_constant("hi");

    // f() calls g()
    let f = m.define_function("perry_fn_m__f", DOUBLE, vec![]);
    let e = f.create_block("entry");
    let r = e.call(DOUBLE, "perry_fn_m__g", &[]);
    e.ret(DOUBLE, &r);
    let g = m.define_function("perry_fn_m__g", DOUBLE, vec![]);
    let e2 = g.create_block("entry");
    e2.ret(DOUBLE, "0.0");

    let units = m.render_codegen_units(2);
    assert_eq!(units.len(), 2, "two functions → two units");

    // Each function defined exactly once across all units.
    let def_f = units
        .iter()
        .filter(|u| u.contains("define double @perry_fn_m__f("))
        .count();
    let def_g = units
        .iter()
        .filter(|u| u.contains("define double @perry_fn_m__g("))
        .count();
    assert_eq!(def_f, 1);
    assert_eq!(def_g, 1);

    // The unit that DEFINES f (and calls g) must DECLARE g.
    let u_with_f = units
        .iter()
        .find(|u| u.contains("define double @perry_fn_m__f("))
        .unwrap();
    assert!(u_with_f.contains("declare double @perry_fn_m__g()"));

    // #7174: each shared global is DEFINED exactly once across units;
    // units that reference it get an `external` declaration instead of a
    // copy. Replicating definitions made per-unit IR grow with the unit
    // count and broke clang's translation-unit limit on real bundles.
    let global_defs = units
        .iter()
        .filter(|u| u.contains("@perry_global_x = global double 0.0"))
        .count();
    assert_eq!(global_defs, 1, "global must be defined in exactly one unit");
    let str_defs = units
        .iter()
        .filter(|u| u.contains("@.str.0 = unnamed_addr constant"))
        .count();
    assert_eq!(str_defs, 1, "string must be defined in exactly one unit");

    // Every unit that mentions the symbol either defines it or declares it
    // external — never neither.
    for u in &units {
        if u.contains("@perry_global_x") {
            assert!(
                u.contains("@perry_global_x = global double 0.0")
                    || u.contains("@perry_global_x = external global double"),
                "referencing unit must define or externally declare the global"
            );
        }
        // Declares are now scoped to what a unit references (the
        // whole-module declaration list was a per-unit floor that
        // splitting could not reduce). A unit that calls the helper must
        // still declare it.
        if u.contains("call void @js_console_log_number") {
            assert!(
                u.contains("declare void @js_console_log_number(double)"),
                "a unit calling the helper must declare it"
            );
        }
        assert!(u.contains("target triple = \"x86_64-pc-windows-msvc\""));
    }
}

/// Every `@sym` a rendered unit DEFINES with linkage the linker treats
/// as strong: not a `private`/`internal` local, not a `linkonce`/`weak`
/// COMDAT the linker folds, not an `external`/`appending` declaration.
/// Two of these with the same name in one link is GNU ld's
/// `multiple definition of ...`.
fn strong_global_definitions(unit: &str) -> Vec<String> {
    unit.lines()
        .filter_map(|line| {
            let name = global_symbol_name(line)?;
            let rhs = line[name.len()..].trim_start().strip_prefix("= ")?;
            let weak_or_local = [
                "private ",
                "internal ",
                "linkonce_odr ",
                "linkonce ",
                "weak_odr ",
                "weak ",
                "external ",
                "appending ",
                "available_externally ",
                "common ",
            ]
            .iter()
            .any(|kw| rhs.starts_with(kw));
            (!weak_or_local).then(|| name.to_string())
        })
        .collect()
}

#[test]
fn split_modules_do_not_export_colliding_string_constants() {
    // A Next.js route bundle compiled to a Linux shared library: five
    // modules large enough to split into codegen units. Splitting
    // promotes every `add_string_constant` global so sibling units can
    // reference it, and on ELF/COFF the owning unit's copy is a plain
    // STRONG global (`make_unique_owner_global`). The `.str.N` counter
    // restarts at 0 per module, so `app-page.runtime.prod.js` and
    // `route.js` both exported `.str.375` — with different contents — and
    // GNU ld refused the final link with 2,188 `multiple definition`
    // errors. (ld64 accepts the Mach-O `linkonce_odr` copies and
    // coalesces them by name, which is worse: one module's bytes silently
    // stand in for the other's.) The fix folds the module prefix into the
    // name, as `strings.rs` already does for `<prefix>_.str.N.bytes`.
    fn split_module(prefix: &str, literal: &str) -> (String, Vec<String>) {
        let mut m = LlModule::new("x86_64-unknown-linux-gnu");
        m.set_symbol_prefix(prefix);
        // The null-guard global is the other unprefixed per-module
        // definition `compile_module` used to mint; it rides the same
        // prefix and the same strong-definition assertion below.
        let null_guard = m.null_guard_global();
        assert_eq!(null_guard, format!("perry_null_guard_zero_{prefix}"));
        m.add_internal_global(&null_guard, I32, "0");
        let (name, len) = m.add_string_constant(literal);
        assert_eq!(len, literal.len());
        // Two functions, each referencing the constant, so a 2-way split
        // has one owning unit and one unit that must resolve it across
        // the unit boundary.
        for fname in ["f", "g"] {
            let f = m.define_function(format!("perry_fn_{prefix}__{fname}"), PTR, vec![]);
            let e = f.create_block("entry");
            let _len = e.safe_load_i32_from_ptr("0");
            e.ret(PTR, &format!("@{name}"));
        }
        let units = m.render_codegen_units(2);
        assert_eq!(units.len(), 2, "two functions → two units");
        (name, units)
    }
    let (name_a, units_a) = split_module("app_page_runtime_prod_js", "alpha");
    let (name_b, units_b) = split_module("route_js", "beta");

    // The name is module-unique (both would have been `.str.0`), and it
    // is what the functions reference.
    assert_eq!(name_a, "app_page_runtime_prod_js_.str.0");
    assert_eq!(name_b, "route_js_.str.0");
    assert_ne!(name_a, name_b);
    for (prefix, name, literal, units) in [
        ("app_page_runtime_prod_js", &name_a, "alpha", &units_a),
        ("route_js", &name_b, "beta", &units_b),
    ] {
        let ty = format!("[{} x i8]", literal.len() + 1);
        let def = format!("@{name} = unnamed_addr constant {ty}");
        let decl = format!("@{name} = external constant {ty}");
        assert_eq!(
            units.iter().filter(|u| u.contains(&def)).count(),
            1,
            "#7174: the constant is DEFINED in exactly one unit"
        );
        assert_eq!(
            units.iter().filter(|u| u.contains(&decl)).count(),
            1,
            "the other unit resolves it through an external declaration"
        );
        for u in units {
            assert!(
                u.contains(&format!("ret ptr @{name}")),
                "both units reference the constant by its prefixed name"
            );
        }
        // The bare per-module names never leak into a link-visible symbol.
        assert!(!units.iter().any(|u| u.contains("@.str.0")));
        assert!(!units.iter().any(
            |u| u.contains("@perry_null_guard_zero ") || u.contains("@perry_null_guard_zero,")
        ));
        let guard_def = format!("@perry_null_guard_zero_{prefix} = global i32 0");
        assert_eq!(
            units.iter().filter(|u| u.contains(&guard_def)).count(),
            1,
            "the null guard is DEFINED (strong, prefixed) in exactly one unit"
        );
    }

    // The GNU ld property: across every unit of both modules, no strong
    // symbol is defined more than once.
    let mut strong: Vec<String> = units_a
        .iter()
        .chain(units_b.iter())
        .flat_map(|u| strong_global_definitions(u))
        .collect();
    assert!(
        strong.iter().any(|s| s == &format!("@{name_a}")),
        "subject is live: the owning unit's copy is a strong ELF definition"
    );
    let n = strong.len();
    strong.sort();
    strong.dedup();
    assert_eq!(
        strong.len(),
        n,
        "a strong global is defined in two units — GNU ld would reject the link"
    );
}

#[test]
fn string_constants_without_a_prefix_keep_the_bare_name() {
    // Single-module fixtures never set a prefix; their `@.str.N` spelling
    // stays exactly as before so nothing downstream shifts.
    let mut m = LlModule::new("x86_64-unknown-linux-gnu");
    let (first, _) = m.add_string_constant("a");
    let (second, _) = m.add_string_constant("b");
    assert_eq!(first, ".str.0");
    assert_eq!(second, ".str.1");
    assert!(m
        .to_ir()
        .contains("@.str.1 = private unnamed_addr constant [2 x i8] c\"b\\00\""));
}

#[test]
fn split_modules_keep_unknown_fallback_wrappers_distinct_at_shared_link() {
    // #8064: splitting promotes internal definitions so sibling units can
    // call them. Before the fallback name was module-scoped, each module's
    // merged object therefore exported the same
    // `__perry_wrap_perry_unknown_func` symbol and the application link
    // failed only after every module had emitted successfully.
    fn split_object(module_prefix: &str) -> Vec<u8> {
        let mut module = LlModule::new(crate::codegen::default_target_triple());
        let wrapper_name = crate::codegen::helpers::unknown_func_wrapper_name(module_prefix);

        let wrapper = module.define_function(
            &wrapper_name,
            DOUBLE,
            vec![
                (I64, "%this_closure".to_string()),
                (DOUBLE, "%a0".to_string()),
                (DOUBLE, "%a1".to_string()),
                (DOUBLE, "%a2".to_string()),
                (DOUBLE, "%a3".to_string()),
                (DOUBLE, "%a4".to_string()),
            ],
        );
        wrapper.linkage = "internal".to_string();
        wrapper
            .create_block("entry")
            .ret(DOUBLE, "0x7FFC000000000001");

        let caller = module.define_function(
            format!("perry_fn_{module_prefix}__use_unknown"),
            DOUBLE,
            vec![],
        );
        let entry = caller.create_block("entry");
        let result = entry.call(
            DOUBLE,
            &wrapper_name,
            &[
                (I64, "0"),
                (DOUBLE, "0.0"),
                (DOUBLE, "0.0"),
                (DOUBLE, "0.0"),
                (DOUBLE, "0.0"),
                (DOUBLE, "0.0"),
            ],
        );
        entry.ret(DOUBLE, &result);

        let units = module.render_codegen_units(2);
        assert_eq!(units.len(), 2, "fixture must exercise split units");
        assert_eq!(
            units
                .iter()
                .filter(|unit| unit.contains(&format!("define double @{wrapper_name}(")))
                .count(),
            1,
            "the internal fallback must be promoted in exactly one split unit"
        );
        crate::linker::compile_units_to_object(&units, None)
            .expect("split module units emit and partial-link")
    }

    let alpha = split_object("alpha_ts");
    let beta = split_object("beta_ts");
    let linked = crate::linker::merge_unit_objects(&[alpha, beta])
        .expect("two split module objects must share a final link");
    assert!(!linked.is_empty(), "shared link must emit an object");
}

#[test]
fn owner_only_global_declares_cross_unit_function_from_initializer() {
    // An unreferenced generated global is retained in unit 0. If its
    // initializer names a function assigned to another unit, unit 0 must
    // still declare that function even though no function body mentions
    // the global. Extern-function ClosureHeaders have exactly this shape.
    let mut m = LlModule::new("x86_64-pc-windows-msvc");

    let big = m.define_function("perry_fn_m__big", DOUBLE, vec![]);
    let block = big.create_block("entry");
    for _ in 0..200 {
        block.call_void("js_noop", &[]);
    }
    block.ret(DOUBLE, "0.0");

    let wrapper = m.define_function(
        "__perry_wrap_extern_dep__value",
        DOUBLE,
        vec![(I64, "%this_closure".to_string())],
    );
    wrapper.linkage = "internal".to_string();
    wrapper.create_block("entry").ret(DOUBLE, "0.0");
    m.add_internal_constant(
        "__perry_extern_closure_dep__value",
        "{ ptr, i32, i32 }",
        "{ ptr @__perry_wrap_extern_dep__value, i32 0, i32 1129074515 }",
    );

    let units = m.render_codegen_units(2);
    let global_unit = units
        .iter()
        .find(|unit| unit.contains("@__perry_extern_closure_dep__value = constant"))
        .expect("one unit must own the closure global");
    assert!(
        !global_unit.contains("define double @__perry_wrap_extern_dep__value("),
        "size balancing should put the small wrapper in the other unit"
    );
    assert!(global_unit.contains("declare double @__perry_wrap_extern_dep__value(i64)"));
}

#[test]
fn mach_o_split_promotes_only_globals_two_units_define() {
    // #9610: `linkonce_odr` is weak-for-linker, and
    // `TargetLoweringObjectFileMachO::SelectSectionForGlobal` routes every
    // weak-for-linker global to the coalesced DATA section before it ever
    // asks whether the initializer is zero. So promoting a
    // `zeroinitializer` global that only ONE unit defines moves it out of
    // zerofill `__DATA,__bss` and writes its zeros into the file — 25.16 MB
    // (8.2%) of the Claude Code binary, all of it per-site inline caches
    // (`[12 x i64] zeroinitializer` then; an 8-byte `ptr null` slot since
    // #9708 — one per property-access site either way, each referenced by
    // exactly one function and so by exactly one unit).
    // Promote only what a link would otherwise see defined twice; ELF/COFF
    // are unaffected either way (their BSS choice ignores linkage).
    let mut m = LlModule::new("arm64-apple-macosx15.0.0");
    m.declare_function("js_ic_touch", VOID, &[PTR]);
    m.add_raw_global(crate::expr::inline_cache_global_definition("perry_ic_m__0"));
    m.add_raw_global(crate::expr::inline_cache_global_definition("perry_ic_m__1"));
    m.add_internal_global("perry_class_keys_m__C", I64, "0");
    m.add_global("perry_class_shape_id_m__C", I32, "0");

    // Two functions, one per unit under a 2-way split. Each touches its
    // own cache; both touch the class-keys global, which therefore needs
    // the linker to fold the two copies onto one storage.
    for (name, ic) in [
        ("perry_fn_m__f", "@perry_ic_m__0"),
        ("perry_fn_m__g", "@perry_ic_m__1"),
    ] {
        let f = m.define_function(name, DOUBLE, vec![]);
        let e = f.create_block("entry");
        e.call_void("js_ic_touch", &[(PTR, ic)]);
        e.call_void("js_ic_touch", &[(PTR, "@perry_class_keys_m__C")]);
        if name == "perry_fn_m__f" {
            e.call_void("js_ic_touch", &[(PTR, "@perry_class_shape_id_m__C")]);
        }
        e.ret(DOUBLE, "0.0");
    }

    let units = m.render_codegen_units(2);
    assert_eq!(units.len(), 2, "two functions → two units");

    for ic in ["@perry_ic_m__0", "@perry_ic_m__1"] {
        let defs: Vec<&String> = units
            .iter()
            .filter(|u| u.contains(&format!("{ic} = private global ptr null")))
            .collect();
        assert_eq!(
            defs.len(),
            1,
            "{ic} is referenced by one function, so exactly one unit defines \
             it — in its original local linkage, which is what keeps it in __bss"
        );
        for u in &units {
            assert!(
                !u.contains(&format!("{ic} = linkonce_odr")),
                "{ic} must not be promoted: no second definition exists to fold"
            );
        }
    }

    // The genuinely shared global still gets the promotion — two strong
    // copies of it in one link is a duplicate-symbol error, and two
    // *local* copies would be two distinct storages for one runtime slot.
    let shared_defs = units
        .iter()
        .filter(|u| u.contains("@perry_class_keys_m__C = linkonce_odr global i64 0"))
        .count();
    assert_eq!(
        shared_defs, 2,
        "a global both units reference is defined in both, folded by linkage"
    );

    // A strong EXTERNAL definition keeps the promotion even at one unit:
    // `linkonce_odr` is what lets ld64 coalesce two modules' same-named
    // globals rather than report a duplicate symbol, and this change is
    // about section placement, not about that.
    let external_defs = units
        .iter()
        .filter(|u| u.contains("@perry_class_shape_id_m__C = linkonce_odr global i32 0"))
        .count();
    assert_eq!(
        external_defs, 1,
        "a link-visible definition stays `linkonce_odr` however few units define it"
    );
}

#[test]
fn duplicate_function_symbol_emitted_once() {
    // Two classes that sanitize to the same name produce a colliding
    // method symbol; it must be emitted once (LLVM rejects redefinition),
    // in both the single-TU and the codegen-unit render paths.
    let mut m = LlModule::new("arm64-apple-macosx15.0.0");
    for _ in 0..2 {
        let f = m.define_function("perry_method_j__foo", DOUBLE, vec![]);
        f.create_block("entry").ret(DOUBLE, "0.0");
    }
    assert_eq!(
        m.to_ir()
            .matches("define double @perry_method_j__foo(")
            .count(),
        1,
        "duplicate symbol must be defined once in to_ir"
    );
    let units = m.render_codegen_units(4);
    let defs: usize = units
        .iter()
        .map(|u| u.matches("define double @perry_method_j__foo(").count())
        .sum();
    assert_eq!(
        defs, 1,
        "duplicate symbol must be defined once across units"
    );
}

#[test]
fn render_codegen_units_balances_by_size_isolating_a_giant_fn() {
    // One huge function + several tiny ones, split into 2 units: greedy
    // size bin-packing must isolate the giant function so it does NOT share
    // a unit with the tiny ones (which would make that unit outsized).
    let mut m = LlModule::new("arm64-apple-macosx15.0.0");
    let big = m.define_function("perry_fn_m__big", DOUBLE, vec![]);
    let be = big.create_block("entry");
    for _ in 0..2000 {
        be.call_void("js_noop", &[]);
    }
    be.ret(DOUBLE, "0.0");
    for k in 0..6 {
        let f = m.define_function(format!("perry_fn_m__small{k}"), DOUBLE, vec![]);
        f.create_block("entry").ret(DOUBLE, "0.0");
    }
    let units = m.render_codegen_units(2);
    assert_eq!(units.len(), 2);
    let big_unit = units
        .iter()
        .find(|u| u.contains("define double @perry_fn_m__big("))
        .unwrap();
    // The giant function's unit holds (essentially) only it — the six small
    // functions land in the other unit to balance bytes.
    let smalls_with_big = (0..6)
        .filter(|k| big_unit.contains(&format!("define double @perry_fn_m__small{k}(")))
        .count();
    assert!(
        smalls_with_big <= 1,
        "giant function should be isolated, not clumped with the small ones (got {smalls_with_big})"
    );
}

#[test]
fn every_module_header_declares_the_same_source_filename() {
    // #8087: the recorded source name is what ELF stores as the object's
    // `STT_FILE` symbol. If a header site omits it, LLVM substitutes the
    // path that reached the assembler — a per-call temp name on the textual
    // path, the in-memory module id on the native one — and the two
    // construction paths can no longer produce byte-identical objects.
    // Mach-O records no such symbol, so a macOS-only check of this would be
    // vacuous; asserting on the emitted TEXT keeps it host-independent.
    let declaration = format!("source_filename = \"{MODULE_SOURCE_NAME}\"");

    let mut m = LlModule::new("x86_64-unknown-linux-gnu");
    for name in ["first", "second"] {
        let f = m.define_function(name, I32, vec![]);
        f.create_block("entry").ret(I32, "0");
    }

    assert!(
        m.to_ir().contains(&declaration),
        "to_ir must declare the source filename:\n{}",
        m.to_ir()
    );

    // A real split: every unit is compiled separately, so every unit
    // prologue needs the declaration, not just the first.
    let units = m.render_codegen_units(2);
    assert_eq!(units.len(), 2, "fixture must exercise a real split");
    for (i, unit) in units.iter().enumerate() {
        assert!(
            unit.contains(&declaration),
            "codegen unit {i} must declare the source filename:\n{unit}"
        );
    }
}

#[cfg(feature = "llvm-inprocess")]
#[test]
fn skeleton_ir_declares_the_same_source_filename_as_to_ir() {
    // The native path parses `skeleton_ir`; the textual path compiles
    // `to_ir`. They must record the same name or #8087 returns.
    let mut m = LlModule::new("x86_64-unknown-linux-gnu");
    let f = m.define_function("only", I32, vec![]);
    f.create_block("entry").ret(I32, "0");

    let declaration = format!("source_filename = \"{MODULE_SOURCE_NAME}\"");
    assert!(m.skeleton_ir().contains(&declaration));
    assert!(m.to_ir().contains(&declaration));
}

#[test]
fn render_codegen_units_single_unit_matches_to_ir() {
    let mut m = LlModule::new("arm64-apple-macosx15.0.0");
    let f = m.define_function("main", I32, vec![]);
    f.create_block("entry").ret(I32, "0");
    assert_eq!(m.render_codegen_units(1), vec![m.to_ir()]);
}

#[test]
fn helper_attr_groups_on_verified_declarations_only() {
    // #6082: allowlisted helpers carry the #2 (pure) / #3 (readonly)
    // group refs; a non-allowlisted helper (js_nanbox_string ALLOCATES)
    // must not; each attributes line is emitted exactly once.
    let mut m = LlModule::new("arm64-apple-macosx15.0.0");
    m.declare_function("js_nanbox_get_pointer", I64, &[DOUBLE]);
    m.declare_function("js_is_truthy", I32, &[DOUBLE]);
    m.declare_function("js_string_compare", I32, &[I64, I64]);
    m.declare_function("js_string_compare_value", I32, &[DOUBLE, DOUBLE]);
    m.declare_function("js_nanbox_string", DOUBLE, &[I64]);
    m.declare_function(
        "js_typed_feedback_numeric_array_index_get_guard",
        I32,
        &[I64, DOUBLE, I32, I32],
    );
    let f = m.define_function("main", I32, vec![]);
    f.create_block("entry").ret(I32, "0");

    let ir = m.to_ir();
    assert!(
        ir.contains("declare i64 @js_nanbox_get_pointer(double) #2"),
        "pure helper must carry the #2 group ref"
    );
    assert!(
        ir.contains("declare i32 @js_is_truthy(double) #3"),
        "readonly helper must carry the #3 group ref"
    );
    assert!(
        ir.contains("declare double @js_nanbox_string(i64)\n"),
        "allocating helper must stay attribute-free"
    );
    assert!(ir.contains("declare i32 @js_string_compare(i64, i64) #3"));
    assert!(ir.contains("declare i32 @js_string_compare_value(double, double)\n"));
    assert!(!ir.contains("js_nanbox_string(i64) #"));
    assert_eq!(
        ir.matches("attributes #2 = { nounwind willreturn readnone }")
            .count(),
        1
    );
    assert_eq!(
        ir.matches("attributes #3 = { nounwind willreturn readonly }")
            .count(),
        1
    );
    // Repsel 4a.0: the array-index guards carry #4 (nounwind willreturn,
    // no memory attribute — the first-touch path rebuilds raw-f64 layout).
    assert!(ir.contains(
        "declare i32 @js_typed_feedback_numeric_array_index_get_guard(i64, double, i32, i32) #4"
    ));
    assert_eq!(
        ir.matches("attributes #4 = { nounwind willreturn }")
            .count(),
        1
    );
    // No setjmp declared → the setjmp-only groups stay out.
    assert!(!ir.contains("attributes #0"));
    assert!(!ir.contains("attributes #1"));
}

#[test]
fn helper_attr_groups_omitted_when_unused() {
    // A module that declares no allowlisted helper must not emit the
    // #2/#3 attributes lines (mirrors the setjmp #0/#1 gating).
    let mut m = LlModule::new("arm64-apple-macosx15.0.0");
    m.declare_function("js_console_log_number", VOID, &[DOUBLE]);
    let f = m.define_function("main", I32, vec![]);
    f.create_block("entry").ret(I32, "0");
    let ir = m.to_ir();
    assert!(!ir.contains("attributes #2"));
    assert!(!ir.contains("attributes #3"));
    assert!(!ir.contains("attributes #4"));
}

#[test]
fn helper_attr_groups_replicated_in_codegen_units() {
    // Every codegen unit re-emits the declaration (with its group ref)
    // and the attributes line, so #2/#3 references resolve per-unit.
    let mut m = LlModule::new("arm64-apple-macosx15.0.0");
    m.declare_function("js_is_truthy", I32, &[DOUBLE]);
    for k in 0..2 {
        let f = m.define_function(format!("perry_fn_m__f{k}"), DOUBLE, vec![]);
        let b = f.create_block("entry");
        // Reference the helper so the declare is genuinely needed: declares
        // are scoped per unit now, and a test whose units never call the
        // helper would assert nothing about its attribute group.
        b.call(I32, "js_is_truthy", &[(DOUBLE, "0.0")]);
        b.ret(DOUBLE, "0.0");
    }
    let units = m.render_codegen_units(2);
    assert_eq!(units.len(), 2);
    for u in &units {
        assert!(u.contains("declare i32 @js_is_truthy(double) #3"));
        assert_eq!(
            u.matches("attributes #3 = { nounwind willreturn readonly }")
                .count(),
            1
        );
    }
}

#[test]
fn hello_world_ir_is_well_formed() {
    let mut m = LlModule::new("arm64-apple-macosx15.0.0");
    m.declare_function("js_console_log_number", VOID, &[DOUBLE]);
    let (_sname, _slen) = m.add_string_constant("hello");

    let f = m.define_function("main", I32, vec![]);
    let entry = f.create_block("entry");
    entry.call_void("js_console_log_number", &[(DOUBLE, "42.0")]);
    entry.ret(I32, "0");

    let ir = m.to_ir();
    assert!(ir.contains("target triple = \"arm64-apple-macosx15.0.0\""));
    assert!(ir.contains("declare void @js_console_log_number(double)"));
    assert!(ir.contains("define i32 @main()"));
    assert!(ir.contains("call void @js_console_log_number(double 42.0)"));
    assert!(ir.contains("ret i32 0"));
}

#[test]
fn declare_is_dropped_when_also_defined() {
    let mut m = LlModule::new("arm64-apple-macosx15.0.0");
    m.declare_function("main", I32, &[]);
    let f = m.define_function("main", I32, vec![]);
    f.create_block("entry").ret(I32, "0");
    let ir = m.to_ir();
    assert!(!ir.contains("declare i32 @main"));
    assert!(ir.contains("define i32 @main"));
}

#[test]
fn split_unit_declaration_uses_local_definition_signature() {
    let mut m = LlModule::new("arm64-apple-macosx15.0.0");

    // Import metadata may register a constructor before its source module
    // is lowered, with a stale arity. Once this module defines the symbol,
    // its definition is authoritative for callers placed in another unit.
    m.declare_function("constructor", DOUBLE, &[DOUBLE]);
    let constructor = m.define_function(
        "constructor",
        DOUBLE,
        vec![
            (DOUBLE, "this_arg".into()),
            (DOUBLE, "arg0".into()),
            (DOUBLE, "arg1".into()),
        ],
    );
    constructor.create_block("entry").ret(DOUBLE, "this_arg");

    let caller = m.define_function("caller", DOUBLE, vec![]);
    let entry = caller.create_block("entry");
    let result = entry.call(
        DOUBLE,
        "constructor",
        &[(DOUBLE, "0.0"), (DOUBLE, "1.0"), (DOUBLE, "2.0")],
    );
    entry.ret(DOUBLE, &result);

    let units = m.render_codegen_units(2);
    let caller_unit = units
        .iter()
        .find(|unit| unit.contains("define double @caller("))
        .expect("caller unit");
    assert!(caller_unit.contains("declare double @constructor(double, double, double)"));
    assert!(!caller_unit.contains("declare double @constructor(double)"));
}

#[test]
fn split_unit_declares_local_function_used_as_pointer_argument() {
    let mut m = LlModule::new("arm64-apple-macosx15.0.0");
    m.declare_function("js_closure_alloc_singleton", I64, &[PTR]);

    let wrapper_name = "__perry_wrap_perry_fn_m___a";
    let wrapper = m.define_function(
        wrapper_name,
        DOUBLE,
        vec![(I64, "%this_closure".into()), (DOUBLE, "%a0".into())],
    );
    wrapper.create_block("entry").ret(DOUBLE, "%a0");

    let init = m.define_function("m__init_body", VOID, vec![]);
    let entry = init.create_block("entry");
    entry.call(
        I64,
        "js_closure_alloc_singleton",
        &[(PTR, &format!("@{wrapper_name}"))],
    );
    entry.ret_void();

    let units = m.render_codegen_units(2);
    let init_unit = units
        .iter()
        .find(|unit| unit.contains("define void @m__init_body("))
        .expect("init unit");
    assert!(!init_unit.contains(&format!("define double @{wrapper_name}(")));
    assert!(init_unit.contains(&format!("declare double @{wrapper_name}(i64, double)")));
}

#[test]
fn string_constant_escapes_nonprintable() {
    let mut m = LlModule::new("arm64-apple-macosx15.0.0");
    let (name, len) = m.add_string_constant("a\nb");
    assert_eq!(name, ".str.0");
    assert_eq!(len, 3);
    let ir = m.to_ir();
    // "a" then \0A then "b" then \00
    assert!(ir.contains("c\"a\\0Ab\\00\""), "got: {}", ir);
}

#[test]
fn gep_unused_helper_imports_compile() {
    // Smoke test that PTR, I64 are re-exported and compile alongside.
    let _ = (PTR, I64);
}
