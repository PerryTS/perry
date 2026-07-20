// Issue #410 + #413 — emitted ArkUI must compile cleanly through ArkTS
// strict mode. These tests pin `serialize_condition` /
// `evaluate_condition` / `collect_compile_time_constants` invariants:
// no `__local_` placeholders, no nested block comments, `__platform__`
// inlined as a numeric literal, literal-only condition folding with
// dead-branch elimination, axis-correct alignment enums, and defensive
// parenthesization of unary/binary sub-expressions.
//
// ----------------------------------------------------------------
// Issue #410 — the three bugs documented in the issue:
//
//   1. Nested block comments — `serialize_condition` fallback
//      returned `"true /* unsupported condition */"` which closed
//      the outer `/* if ((...)) */` wrapper early on line 82.
//
//   2. `__local_N` undeclared identifiers — `serialize_condition`
//      emitted `__local_<id>` for `Expr::LocalGet`, leaking into
//      the emitted ArkTS as `if (__local_2) { ... }`.
//
//   3. `__platform__` references — once Bug 2 resolves through
//      bindings, `__platform__ === N` surfaced in emitted code
//      where `__platform__` isn't declared on the page struct.
//
// The fix lives in `serialize_condition` + `collect_compile_time_constants`.
// ----------------------------------------------------------------
use super::*;

#[test]
fn issue_410_serialize_condition_fallback_has_no_block_comment_close() {
    // The fallback (any unrecognized condition shape) must never
    // produce a `*/` substring — which would close the outer
    // `/* if ((...)) */` wrapper used by emit_modifier_mutations.
    let bindings = HashMap::new();
    let consts = HashMap::new();
    // A Call expression isn't recognized by serialize_condition's
    // match arms, so it lands in the fallback.
    let unrecognized = Expr::Call {
        callee: Box::new(Expr::LocalGet(99)),
        args: vec![],
        type_args: vec![],
        byte_offset: 0,
    };
    let s = serialize_condition(&unrecognized, &bindings, &consts);
    assert!(
        !s.contains("*/"),
        "fallback emitted */ — bug 1 regressed: {}",
        s
    );
    assert_eq!(
        s, "true",
        "fallback should be the literal 'true', got: {}",
        s
    );
}

#[test]
fn issue_410_local_get_resolves_through_bindings_not_placeholder() {
    // `let mobile = (props.screen === 'mobile')` — when a condition
    // references `mobile`, serialize_condition resolves the local
    // back to the init expression. The init contains a PropertyGet
    // on an unresolvable LocalGet — post-v0.5.489 the cleanly-
    // serializable gate at the top of serialize_condition catches
    // this and degrades the entire condition to `true` (the
    // unresolvable-LocalGet heuristic, lifted to root level).
    // Pre-fix this emitted `true.screen === 'mobile'` which ArkTS
    // strict-mode rejected with "Property 'screen' does not exist
    // on type 'true'".
    //
    // The original test name still applies: the emitted source
    // must NOT contain `__local_N` placeholder text. The exact
    // shape changed from "resolved condition" to "true" once the
    // root-level gate landed.
    let mobile_id: LocalId = 5;
    let init = Expr::Compare {
        op: perry_hir::ir::CompareOp::Eq,
        left: Box::new(Expr::PropertyGet {
            byte_offset: 0,
            object: Box::new(Expr::LocalGet(99)), // unresolvable
            property: "screen".to_string(),
        }),
        right: Box::new(Expr::String("mobile".into())),
    };
    let mut bindings = HashMap::new();
    bindings.insert(mobile_id, init);
    let consts = HashMap::new();
    let s = serialize_condition(&Expr::LocalGet(mobile_id), &bindings, &consts);
    assert!(
        !s.contains("__local_"),
        "emitted __local_ placeholder — bug 2 regressed: {}",
        s
    );
    assert_eq!(
        s, "true",
        "PropertyGet on unresolvable LocalGet should degrade to 'true', got: {}",
        s
    );
}

#[test]
fn issue_410_unresolvable_local_get_degrades_to_true_not_placeholder() {
    // A LocalGet that's not in bindings (e.g., closure-captured or
    // loop-mutated) degrades to `true` rather than leaking
    // `__local_N` into emitted ArkTS.
    let bindings = HashMap::new();
    let consts = HashMap::new();
    let s = serialize_condition(&Expr::LocalGet(42), &bindings, &consts);
    assert_eq!(
        s, "true",
        "unresolvable LocalGet should degrade to 'true', got: {}",
        s
    );
}

#[test]
fn issue_410_platform_constant_inlines_as_number_literal() {
    // `__platform__ === 9` should serialize with the literal 9
    // inlined (since this codegen is harmonyos-only). Without the
    // compile_time_consts inlining, the LocalGet would resolve via
    // `bindings` and find no entry (declare-const has init: None),
    // ultimately leaking `__platform__` into emitted ArkTS.
    let plat_id: LocalId = 7;
    let bindings = HashMap::new();
    let mut consts = HashMap::new();
    consts.insert(plat_id, 9.0);
    let cmp = Expr::Compare {
        op: perry_hir::ir::CompareOp::Eq,
        left: Box::new(Expr::LocalGet(plat_id)),
        right: Box::new(Expr::Integer(9)),
    };
    let s = serialize_condition(&cmp, &bindings, &consts);
    assert!(
        !s.contains("__platform__"),
        "platform constant leaked: {}",
        s
    );
    assert!(
        !s.contains("__local_"),
        "platform local leaked as placeholder: {}",
        s
    );
    // 9 === 9 — both sides should be the literal 9.
    assert!(s.contains("9"), "expected platform value 9, got: {}", s);
}

#[test]
fn issue_410_collect_compile_time_constants_picks_up_declare_const() {
    // `declare const __platform__: number;` lowers to
    // `Stmt::Let { name: "__platform__", init: None }`. The collector
    // must recognize this canonical shape and assign 9.0 (harmonyos).
    let init = vec![declare_const(11, "__platform__")];
    let map = collect_compile_time_constants(&init);
    assert_eq!(map.get(&11), Some(&9.0));
}

#[test]
fn issue_410_conditional_addchild_emits_valid_arkts_if_block() {
    // The ternary-style shape from #410's "Implementation steps":
    // `if (mobile) widgetAddChild(parent, phone) else widgetAddChild(parent, desktop)`
    // where `mobile` is a top-level binding referencing `__platform__`.
    //
    // Post-#413, `__platform__ === 9` constant-folds to `true` (this
    // codegen path is harmonyos-only, where __platform__ inlines to
    // 9), so the entire `if/else` block evaporates and ONLY the
    // then-branch's `Button('phone')` is emitted as an
    // unconditional child. ArkTS strict-mode previously rejected
    // `if (9 === 9) { ... }` with a no-overlap warning; this
    // dead-branch elimination keeps the source legal.
    let mut m = empty_module();
    let plat_id: LocalId = 1;
    let mobile_id: LocalId = 2;
    let parent_id: LocalId = 3;
    let phone_id: LocalId = 4;
    let desktop_id: LocalId = 5;
    m.init.push(declare_const(plat_id, "__platform__"));
    // let mobile = (__platform__ === 9);
    m.init.push(let_widget(
        mobile_id,
        "mobile",
        Expr::Compare {
            op: perry_hir::ir::CompareOp::Eq,
            left: Box::new(Expr::LocalGet(plat_id)),
            right: Box::new(Expr::Integer(9)),
        },
    ));
    m.init.push(let_widget(
        parent_id,
        "parent",
        nmc("HStack", vec![Expr::Number(0.0), Expr::Array(vec![])]),
    ));
    m.init.push(let_widget(
        phone_id,
        "phoneToolbar",
        nmc("Button", vec![Expr::String("phone".into())]),
    ));
    m.init.push(let_widget(
        desktop_id,
        "desktopToolbar",
        nmc("Button", vec![Expr::String("desktop".into())]),
    ));
    m.init.push(Stmt::If {
        condition: Expr::LocalGet(mobile_id),
        then_branch: vec![mutator_stmt(
            "widgetAddChild",
            vec![Expr::LocalGet(parent_id), Expr::LocalGet(phone_id)],
        )],
        else_branch: Some(vec![mutator_stmt(
            "widgetAddChild",
            vec![Expr::LocalGet(parent_id), Expr::LocalGet(desktop_id)],
        )]),
    });
    m.init.push(app_with_body(Expr::LocalGet(parent_id)));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    let src = &r.ets_source;
    assert!(
        !src.contains("__local_"),
        "emitted source contains __local_ — bug 2 regressed:\n{}",
        src
    );
    assert!(
        !src.contains("__platform__"),
        "emitted source contains __platform__ — bug 3 regressed:\n{}",
        src
    );
    assert!(
        !src.contains("/* unsupported condition */"),
        "emitted source contains the bug-1 diagnostic comment:\n{}",
        src
    );
    // #413: dead-branch elimination — `9 === 9` folds to `true`, so
    // there's no `if (...)` block at all in the emitted source for
    // this widget; the then-branch's Button is unconditional.
    assert!(
        !src.contains("if (9 === 9)"),
        "literal-only `if (9 === 9)` must be folded out (#413):\n{}",
        src
    );
    assert!(
        src.contains("Button('phone')"),
        "missing then-branch (live after fold):\n{}",
        src
    );
    assert!(
        !src.contains("Button('desktop')"),
        "else-branch should be dead after fold (#413):\n{}",
        src
    );
    // Also pin: no nested */ pattern that would cascade-break ArkTS
    // parsing (Bug 1). We scan for any /* ... */ wrappers and
    // check that the opening `/*` only ever pairs with one `*/`.
    assert_no_nested_block_comments(src);
}

#[test]
fn issue_410_conditional_modifier_chain_has_no_nested_block_comments() {
    // The procedural-mutation-with-conditional-modifier shape from
    // #410. Build a card with an unconditional modifier chain plus
    // a conditional one inside an `if` whose predicate would have
    // surfaced as `__local_N` pre-fix and broken on the fallback's
    // `*/` substring. Post-fix, both the predicate and the
    // surrounding /* if (...) */ comment must be safe.
    let mut m = empty_module();
    let card_id: LocalId = 200;
    let cond_id: LocalId = 201;
    // let isLarge = (something_unsupported_call())
    // → fallback to `true` post-fix; pre-fix would have emitted
    //   the nested-comment cascade.
    m.init.push(let_widget(
        cond_id,
        "isLarge",
        Expr::Call {
            callee: Box::new(Expr::LocalGet(999)),
            args: vec![],
            type_args: vec![],
            byte_offset: 0,
        },
    ));
    m.init.push(let_widget(
        card_id,
        "card",
        nmc("VStack", vec![Expr::Array(vec![])]),
    ));
    m.init.push(mutator_stmt(
        "widgetSetBackgroundColor",
        vec![
            Expr::LocalGet(card_id),
            Expr::Number(0.5),
            Expr::Number(0.5),
            Expr::Number(0.5),
            Expr::Number(1.0),
        ],
    ));
    // Conditional padding mutator — emits as `/* if ((...)) */ .padding(...)`.
    m.init.push(Stmt::If {
        condition: Expr::LocalGet(cond_id),
        then_branch: vec![mutator_stmt(
            "setPadding",
            vec![
                Expr::LocalGet(card_id),
                Expr::Number(16.0),
                Expr::Number(16.0),
                Expr::Number(16.0),
                Expr::Number(16.0),
            ],
        )],
        else_branch: None,
    });
    m.init.push(app_with_body(Expr::LocalGet(card_id)));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    let src = &r.ets_source;
    assert!(
        !src.contains("__local_"),
        "emitted source contains __local_ — bug 2 regressed:\n{}",
        src
    );
    assert!(
        !src.contains("/* unsupported condition */"),
        "emitted source contains the bug-1 diagnostic comment:\n{}",
        src
    );
    // The unconditional background modifier still applies.
    assert!(
        src.contains(".backgroundColor("),
        "expected unconditional background:\n{}",
        src
    );
    // Bug 1 acceptance bar: no nested /* ... */ patterns anywhere.
    assert_no_nested_block_comments(src);
}

// ─────────────────────────────────────────────────────────────────
// Issue #413 — emitted ArkUI must compile through ArkTS strict mode.
//
// Two bugs documented in the issue:
//
//   1. Literal-only comparisons in conditions: with `__platform__`
//      inlined to 9 (harmonyos codegen path) and bindings resolved,
//      a condition like `__platform__ === 1` serialized to
//      `9 === 1`, and ArkTS rejected `if (9 === 1) { ... }` with
//      a "no overlap" error. Fix: constant-fold via
//      `evaluate_condition` and drop dead branches at harvest time.
//      Operator-precedence: when a binding's init expression is
//      Binary/Logical/Unary and gets spliced into another such
//      expression, parens prevent precedence inversion (e.g.
//      `!isIOS` becoming `!9` then `=== 1` rather than
//      `!(9 === 1)`).
//
//   2. Cross-axis alignment enum on HStack: ArkUI Row's cross-axis
//      is vertical (uses `VerticalAlign`), Column's is horizontal
//      (uses `HorizontalAlign`). v0.5.480's `stackSetAlignment`
//      always emitted `HorizontalAlign.X`, which ArkTS rejected
//      for HStack with a type-mismatch error.
// ─────────────────────────────────────────────────────────────────

#[test]
fn issue_413_evaluate_condition_folds_literal_eq_false() {
    // 1 === 2 → Some(false)
    let bindings = HashMap::new();
    let consts = HashMap::new();
    let cmp = Expr::Compare {
        op: perry_hir::ir::CompareOp::Eq,
        left: Box::new(Expr::Integer(1)),
        right: Box::new(Expr::Integer(2)),
    };
    assert_eq!(evaluate_condition(&cmp, &bindings, &consts), Some(false));
}

#[test]
fn issue_413_evaluate_condition_folds_literal_eq_true() {
    // 1 === 1 → Some(true)
    let bindings = HashMap::new();
    let consts = HashMap::new();
    let cmp = Expr::Compare {
        op: perry_hir::ir::CompareOp::Eq,
        left: Box::new(Expr::Integer(1)),
        right: Box::new(Expr::Integer(1)),
    };
    assert_eq!(evaluate_condition(&cmp, &bindings, &consts), Some(true));
}

#[test]
fn issue_413_evaluate_condition_returns_none_for_runtime_value() {
    // PropertyGet on an unresolved local is non-foldable.
    let bindings = HashMap::new();
    let consts = HashMap::new();
    let prop = Expr::PropertyGet {
        byte_offset: 0,
        object: Box::new(Expr::LocalGet(99)),
        property: "isMobile".to_string(),
    };
    assert_eq!(evaluate_condition(&prop, &bindings, &consts), None);
}

#[test]
fn issue_413_evaluate_condition_resolves_through_compile_time_consts() {
    // __platform__ === 9 (with __platform__ as a compile-time
    // constant inlined to 9.0) → Some(true).
    let plat_id: LocalId = 7;
    let bindings = HashMap::new();
    let mut consts = HashMap::new();
    consts.insert(plat_id, 9.0);
    let cmp = Expr::Compare {
        op: perry_hir::ir::CompareOp::Eq,
        left: Box::new(Expr::LocalGet(plat_id)),
        right: Box::new(Expr::Integer(9)),
    };
    assert_eq!(evaluate_condition(&cmp, &bindings, &consts), Some(true));
}

#[test]
fn issue_413_evaluate_condition_logical_or_short_circuits() {
    // (9 === 1) || (9 === 9) → Some(true) via short-circuit.
    let plat_id: LocalId = 7;
    let bindings = HashMap::new();
    let mut consts = HashMap::new();
    consts.insert(plat_id, 9.0);
    let cmp = Expr::Logical {
        op: perry_hir::ir::LogicalOp::Or,
        left: Box::new(Expr::Compare {
            op: perry_hir::ir::CompareOp::Eq,
            left: Box::new(Expr::LocalGet(plat_id)),
            right: Box::new(Expr::Integer(1)),
        }),
        right: Box::new(Expr::Compare {
            op: perry_hir::ir::CompareOp::Eq,
            left: Box::new(Expr::LocalGet(plat_id)),
            right: Box::new(Expr::Integer(9)),
        }),
    };
    assert_eq!(evaluate_condition(&cmp, &bindings, &consts), Some(true));
}

#[test]
fn issue_413_evaluate_condition_unary_not_negates_literal() {
    // !true → Some(false)
    let bindings = HashMap::new();
    let consts = HashMap::new();
    let neg = Expr::Unary {
        op: perry_hir::ir::UnaryOp::Not,
        operand: Box::new(Expr::Bool(true)),
    };
    assert_eq!(evaluate_condition(&neg, &bindings, &consts), Some(false));
}

#[test]
fn issue_413_literal_only_if_block_drops_dead_branch_emits_only_then() {
    // if (1 === 2) widgetAddChild(parent, btn_a) — 1 === 2 folds to
    // false, so the dead then-branch is dropped and nothing is
    // appended. The parent stays empty.
    let mut m = empty_module();
    let parent_id: LocalId = 80;
    let btn_a_id: LocalId = 81;
    m.init.push(let_widget(
        parent_id,
        "parent",
        nmc("VStack", vec![Expr::Array(vec![])]),
    ));
    m.init.push(let_widget(
        btn_a_id,
        "btn_a",
        nmc("Button", vec![Expr::String("dead".into())]),
    ));
    m.init.push(Stmt::If {
        condition: Expr::Compare {
            op: perry_hir::ir::CompareOp::Eq,
            left: Box::new(Expr::Integer(1)),
            right: Box::new(Expr::Integer(2)),
        },
        then_branch: vec![mutator_stmt(
            "widgetAddChild",
            vec![Expr::LocalGet(parent_id), Expr::LocalGet(btn_a_id)],
        )],
        else_branch: None,
    });
    m.init.push(app_with_body(Expr::LocalGet(parent_id)));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    let src = &r.ets_source;
    assert!(
        !src.contains("Button('dead')"),
        "dead-branch button should not be emitted:\n{}",
        src
    );
    // ArkTS strict-mode would have rejected `if (1 === 2)`. After
    // the fold it never appears in the source.
    assert!(
        !src.contains("if (1 === 2)") && !src.contains("if (1===2)"),
        "literal-only `if` predicate must be folded:\n{}",
        src
    );
}

#[test]
fn issue_413_literal_only_if_block_keeps_then_inlines_no_if_wrapper() {
    // if (1 === 1) widgetAddChild(parent, btn_a) — 1 === 1 folds to
    // true, so the live then-branch's child is inlined as an
    // unconditional sibling and no `if (...)` wrapper is emitted.
    let mut m = empty_module();
    let parent_id: LocalId = 82;
    let btn_a_id: LocalId = 83;
    m.init.push(let_widget(
        parent_id,
        "parent",
        nmc("VStack", vec![Expr::Array(vec![])]),
    ));
    m.init.push(let_widget(
        btn_a_id,
        "btn_a",
        nmc("Button", vec![Expr::String("live".into())]),
    ));
    m.init.push(Stmt::If {
        condition: Expr::Compare {
            op: perry_hir::ir::CompareOp::Eq,
            left: Box::new(Expr::Integer(1)),
            right: Box::new(Expr::Integer(1)),
        },
        then_branch: vec![mutator_stmt(
            "widgetAddChild",
            vec![Expr::LocalGet(parent_id), Expr::LocalGet(btn_a_id)],
        )],
        else_branch: None,
    });
    m.init.push(app_with_body(Expr::LocalGet(parent_id)));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    let src = &r.ets_source;
    assert!(
        src.contains("Button('live')"),
        "live-branch button must be emitted:\n{}",
        src
    );
    assert!(
        !src.contains("if (1 === 1)") && !src.contains("if (1===1)"),
        "literal-only `if` predicate must be folded out of the source:\n{}",
        src
    );
}

#[test]
fn issue_413_platform_const_eq_drops_dead_branch_in_addchild() {
    // Same shape as #410's repro but with __platform__ === 1 (the
    // mobile-style check that's false on harmonyos where
    // __platform__ === 9). Pre-#413 this serialized to
    // `if (9 === 1) { Button('phone') } else { Button('desktop') }`
    // which ArkTS rejected. Post-#413 it folds to `false` and only
    // the desktop branch survives.
    let mut m = empty_module();
    let plat_id: LocalId = 1;
    let parent_id: LocalId = 2;
    let phone_id: LocalId = 3;
    let desktop_id: LocalId = 4;
    m.init.push(declare_const(plat_id, "__platform__"));
    m.init.push(let_widget(
        parent_id,
        "parent",
        nmc("HStack", vec![Expr::Number(0.0), Expr::Array(vec![])]),
    ));
    m.init.push(let_widget(
        phone_id,
        "phoneToolbar",
        nmc("Button", vec![Expr::String("phone".into())]),
    ));
    m.init.push(let_widget(
        desktop_id,
        "desktopToolbar",
        nmc("Button", vec![Expr::String("desktop".into())]),
    ));
    m.init.push(Stmt::If {
        condition: Expr::Compare {
            op: perry_hir::ir::CompareOp::Eq,
            left: Box::new(Expr::LocalGet(plat_id)),
            right: Box::new(Expr::Integer(1)),
        },
        then_branch: vec![mutator_stmt(
            "widgetAddChild",
            vec![Expr::LocalGet(parent_id), Expr::LocalGet(phone_id)],
        )],
        else_branch: Some(vec![mutator_stmt(
            "widgetAddChild",
            vec![Expr::LocalGet(parent_id), Expr::LocalGet(desktop_id)],
        )]),
    });
    m.init.push(app_with_body(Expr::LocalGet(parent_id)));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    let src = &r.ets_source;
    assert!(
        !src.contains("Button('phone')"),
        "dead then-branch (9 === 1 is false) must be dropped:\n{}",
        src
    );
    assert!(
        src.contains("Button('desktop')"),
        "live else-branch must be emitted:\n{}",
        src
    );
    assert!(
        !src.contains("if (9 === 1)") && !src.contains("if (9===1)"),
        "literal `if (9 === 1)` must not appear:\n{}",
        src
    );
}

#[test]
fn issue_413_local_get_resolves_through_binding_to_platform_compare() {
    // let mobile = __platform__ === 1;  (binding)
    // if (mobile) widgetAddChild(parent, phone) else widgetAddChild(parent, desktop);
    // Should fold the same as the inlined comparison: `mobile`
    // resolves to `9 === 1` which is `false`, so only the desktop
    // branch survives.
    let mut m = empty_module();
    let plat_id: LocalId = 1;
    let mobile_id: LocalId = 2;
    let parent_id: LocalId = 3;
    let phone_id: LocalId = 4;
    let desktop_id: LocalId = 5;
    m.init.push(declare_const(plat_id, "__platform__"));
    m.init.push(let_widget(
        mobile_id,
        "mobile",
        Expr::Compare {
            op: perry_hir::ir::CompareOp::Eq,
            left: Box::new(Expr::LocalGet(plat_id)),
            right: Box::new(Expr::Integer(1)),
        },
    ));
    m.init.push(let_widget(
        parent_id,
        "parent",
        nmc("HStack", vec![Expr::Number(0.0), Expr::Array(vec![])]),
    ));
    m.init.push(let_widget(
        phone_id,
        "btn_phone",
        nmc("Button", vec![Expr::String("phone".into())]),
    ));
    m.init.push(let_widget(
        desktop_id,
        "btn_desktop",
        nmc("Button", vec![Expr::String("desktop".into())]),
    ));
    m.init.push(Stmt::If {
        condition: Expr::LocalGet(mobile_id),
        then_branch: vec![mutator_stmt(
            "widgetAddChild",
            vec![Expr::LocalGet(parent_id), Expr::LocalGet(phone_id)],
        )],
        else_branch: Some(vec![mutator_stmt(
            "widgetAddChild",
            vec![Expr::LocalGet(parent_id), Expr::LocalGet(desktop_id)],
        )]),
    });
    m.init.push(app_with_body(Expr::LocalGet(parent_id)));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    let src = &r.ets_source;
    assert!(
        !src.contains("Button('phone')"),
        "dead then-branch (mobile = 9 === 1 = false) must be dropped:\n{}",
        src
    );
    assert!(
        src.contains("Button('desktop')"),
        "live else-branch must be emitted:\n{}",
        src
    );
}

#[test]
fn issue_413_hstack_set_alignment_emits_vertical_align_enum() {
    // HStack (= ArkUI Row) cross-axis is vertical: must use
    // `VerticalAlign.Start`, not `HorizontalAlign.Start`.
    let mut m = empty_module();
    let id: LocalId = 100;
    m.init.push(let_widget(
        id,
        "row",
        nmc("HStack", vec![Expr::Number(0.0), Expr::Array(vec![])]),
    ));
    m.init.push(mutator_stmt(
        "stackSetAlignment",
        vec![Expr::LocalGet(id), Expr::Number(0.0)], // Start
    ));
    m.init.push(app_with_body(Expr::LocalGet(id)));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    let src = &r.ets_source;
    // v0.5.484 follow-up — `VerticalAlign` enum doesn't have a `Start`
    // member (only `Top` / `Center` / `Bottom`). Pre-v0.5.484 this
    // assertion pinned the broken `VerticalAlign.Start` shape that
    // ArkTS strict-mode rejected. Now the value-name is axis-correct.
    assert!(
        src.contains(".alignItems(VerticalAlign.Top)"),
        "HStack + start (0) must emit VerticalAlign.Top:\n{}",
        src
    );
    assert!(
        !src.contains("HorizontalAlign"),
        "HStack must NOT emit HorizontalAlign:\n{}",
        src
    );
}

#[test]
fn issue_413_vstack_set_alignment_emits_horizontal_align_enum() {
    // VStack (= ArkUI Column) cross-axis is horizontal: must use
    // `HorizontalAlign.Start`. Regression-pin to ensure the new
    // axis-aware emit didn't accidentally flip the VStack arm.
    let mut m = empty_module();
    let id: LocalId = 101;
    m.init.push(let_widget(
        id,
        "col",
        nmc("VStack", vec![Expr::Array(vec![])]),
    ));
    m.init.push(mutator_stmt(
        "stackSetAlignment",
        vec![Expr::LocalGet(id), Expr::Number(0.0)], // Start
    ));
    m.init.push(app_with_body(Expr::LocalGet(id)));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    let src = &r.ets_source;
    assert!(
        src.contains(".alignItems(HorizontalAlign.Start)"),
        "VStack must emit HorizontalAlign.Start:\n{}",
        src
    );
    assert!(
        !src.contains("VerticalAlign"),
        "VStack must NOT emit VerticalAlign:\n{}",
        src
    );
}

#[test]
fn issue_413_serialize_condition_parenthesizes_unary_of_compare() {
    // !mobile where mobile = (__platform__ === 1).
    // After binding-resolution, the unary `!` operates on the
    // serialized comparison. Without defensive parenthesization,
    // the result `!9 === 1` parses as `(!9) === 1` (false === 1 →
    // bool→num coercion → 0 === 1 → false) instead of the
    // intended `!(9 === 1)` (== !false → true). The parens fix
    // pins the precedence.
    let plat_id: LocalId = 7;
    let mobile_id: LocalId = 8;
    let bindings = {
        let mut b = HashMap::new();
        b.insert(
            mobile_id,
            Expr::Compare {
                op: perry_hir::ir::CompareOp::Eq,
                left: Box::new(Expr::LocalGet(plat_id)),
                right: Box::new(Expr::Integer(1)),
            },
        );
        b
    };
    let mut consts = HashMap::new();
    consts.insert(plat_id, 9.0);
    let neg = Expr::Unary {
        op: perry_hir::ir::UnaryOp::Not,
        operand: Box::new(Expr::LocalGet(mobile_id)),
    };
    let s = serialize_condition(&neg, &bindings, &consts);
    // Must contain `!(...)` where `...` covers the comparison —
    // i.e. the `(` immediately after `!`. The internal contents
    // are `9 === 1` (whitespace from the operator string) so the
    // exact substring is `!(9 === 1)`.
    assert!(
        s.contains("!(9 === 1)") || s.contains("!(9===1)"),
        "expected unary-not to wrap binding-resolved comparison in parens, got: {}",
        s
    );
    // Negative-pin: the unparenthesized form `!9 === 1` must NOT
    // appear (which would parse as `(!9) === 1`).
    assert!(
        !s.contains("!9 === 1") && !s.contains("!9===1"),
        "unparenthesized `!9 === 1` precedence-inversion bug regressed: {}",
        s
    );
}

#[test]
fn issue_413_serialize_condition_parenthesizes_or_chain_with_unary() {
    // mobile = __platform__ === 1 || __platform__ === 2 || (!isIOS && x)
    // where isIOS = __platform__ === 1 (so isIOS = false, and
    // !isIOS = true), and x is an unresolved PropertyGet so the
    // whole chain doesn't fold to a literal — it stays a runtime
    // condition. The serialized chain must parenthesize each
    // sub-Binary/Unary so precedence can't invert.
    let plat_id: LocalId = 7;
    let isios_id: LocalId = 9;
    let mut bindings = HashMap::new();
    bindings.insert(
        isios_id,
        Expr::Compare {
            op: perry_hir::ir::CompareOp::Eq,
            left: Box::new(Expr::LocalGet(plat_id)),
            right: Box::new(Expr::Integer(1)),
        },
    );
    let mut consts = HashMap::new();
    consts.insert(plat_id, 9.0);
    // (__platform__ === 1) || (__platform__ === 2) || (!isIOS && something)
    let chain = Expr::Logical {
        op: perry_hir::ir::LogicalOp::Or,
        left: Box::new(Expr::Logical {
            op: perry_hir::ir::LogicalOp::Or,
            left: Box::new(Expr::Compare {
                op: perry_hir::ir::CompareOp::Eq,
                left: Box::new(Expr::LocalGet(plat_id)),
                right: Box::new(Expr::Integer(1)),
            }),
            right: Box::new(Expr::Compare {
                op: perry_hir::ir::CompareOp::Eq,
                left: Box::new(Expr::LocalGet(plat_id)),
                right: Box::new(Expr::Integer(2)),
            }),
        }),
        right: Box::new(Expr::Unary {
            op: perry_hir::ir::UnaryOp::Not,
            operand: Box::new(Expr::LocalGet(isios_id)),
        }),
    };
    let s = serialize_condition(&chain, &bindings, &consts);
    // The buggy serialization documented in the issue:
    //     `9 === 1 || 9 === 2 || !9 === 1`
    // (note `!9 === 1` parses as `(!9) === 1`). Post-fix this
    // specific substring must NOT appear.
    assert!(
        !s.contains("!9 === 1") && !s.contains("!9===1"),
        "precedence-inverted `!9 === 1` regressed: {}",
        s
    );
    // Unary `!` must wrap the resolved comparison in parens.
    // (v0.5.489 note: dropped the `&& <unresolvable PropertyGet>`
    // tail from the chain — the new cleanly-serializable gate at
    // the root of serialize_condition would have degraded the whole
    // condition to `true` once any sub-expression hits an
    // unresolvable PropertyGet. The unary-paren behavior is still
    // exercised by the now-resolvable chain.)
    assert!(
        s.contains("!(9 === 1)") || s.contains("!(9===1)"),
        "expected unary-not paren-wrap: {}",
        s
    );
}

#[test]
fn issue_490_unfoldable_unresolvable_condition_walks_only_then_branch() {
    // v0.5.490: when a condition is unfoldable AND not cleanly
    // serializable, dead-branch elim picks the then-branch. The
    // pre-v0.5.490 behavior emitted both branches under `if (true)
    // {...} else {...}` — Mango's `connectionNames.length === 0`
    // exposed this as the "+ New Connection" duplicate-content bug.
    let mut m = empty_module();
    let parent_id: LocalId = 110;
    let a_id: LocalId = 111;
    let b_id: LocalId = 112;
    m.init.push(let_widget(
        parent_id,
        "parent",
        nmc("VStack", vec![Expr::Array(vec![])]),
    ));
    m.init.push(let_widget(
        a_id,
        "btn_a",
        nmc("Button", vec![Expr::String("a".into())]),
    ));
    m.init.push(let_widget(
        b_id,
        "btn_b",
        nmc("Button", vec![Expr::String("b".into())]),
    ));
    m.init.push(Stmt::If {
        condition: Expr::PropertyGet {
            byte_offset: 0,
            object: Box::new(Expr::LocalGet(9999)),
            property: "isMobile".to_string(),
        },
        then_branch: vec![mutator_stmt(
            "widgetAddChild",
            vec![Expr::LocalGet(parent_id), Expr::LocalGet(a_id)],
        )],
        else_branch: Some(vec![mutator_stmt(
            "widgetAddChild",
            vec![Expr::LocalGet(parent_id), Expr::LocalGet(b_id)],
        )]),
    });
    m.init.push(app_with_body(Expr::LocalGet(parent_id)));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    let src = &r.ets_source;
    // Then-branch only — heuristic pick.
    assert!(
        src.contains("Button('a')"),
        "then-branch must render:\n{}",
        src
    );
    assert!(
        !src.contains("Button('b')"),
        "else-branch must NOT render (dead-branch elim):\n{}",
        src
    );
}
