// Issue #408 procedural-mutation tracking tests: widgetAddChild /
// scrollviewSetChild / setPadding / tooltips / clear-children / hidden /
// match-parent-size / distribution+alignment / text-styling mutators,
// plus the unrecognized-mutator comment behavior and the Mango composite.
use super::*;

#[test]
fn issue_408_hstack_with_widget_add_child_appends_children() {
    // const toolbar = HStack(0, []);
    // widgetAddChild(toolbar, button1);
    // widgetAddChild(toolbar, button2);
    // App({body: toolbar});
    let mut m = empty_module();
    let toolbar_id: LocalId = 10;
    let btn_a_id: LocalId = 11;
    let btn_b_id: LocalId = 12;
    m.init.push(let_widget(
        toolbar_id,
        "toolbar",
        nmc("HStack", vec![Expr::Number(0.0), Expr::Array(vec![])]),
    ));
    m.init.push(let_widget(
        btn_a_id,
        "btn_a",
        nmc("Button", vec![Expr::String("A".into())]),
    ));
    m.init.push(let_widget(
        btn_b_id,
        "btn_b",
        nmc("Button", vec![Expr::String("B".into())]),
    ));
    m.init.push(mutator_stmt(
        "widgetAddChild",
        vec![Expr::LocalGet(toolbar_id), Expr::LocalGet(btn_a_id)],
    ));
    m.init.push(mutator_stmt(
        "widgetAddChild",
        vec![Expr::LocalGet(toolbar_id), Expr::LocalGet(btn_b_id)],
    ));
    m.init.push(app_with_body(Expr::LocalGet(toolbar_id)));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(
        r.ets_source.contains("Row({ space: 0 })"),
        "expected Row container:\n{}",
        r.ets_source
    );
    // Both children must appear inside the body. They show up after
    // the explicit empty array's children (none) so they're the only
    // contents of Row.
    assert!(
        r.ets_source.contains("Button('A')"),
        "missing Button A:\n{}",
        r.ets_source
    );
    assert!(
        r.ets_source.contains("Button('B')"),
        "missing Button B:\n{}",
        r.ets_source
    );
    // Order: A appears before B in the source.
    let pos_a = r.ets_source.find("Button('A')").unwrap();
    let pos_b = r.ets_source.find("Button('B')").unwrap();
    assert!(pos_a < pos_b, "child order swapped:\n{}", r.ets_source);
}

#[test]
fn issue_408_scrollview_set_child_replaces_body() {
    // const screen = ScrollView();
    // const content = VStack([Text("hello")]);
    // scrollviewSetChild(screen, content);
    // App({body: screen});
    let mut m = empty_module();
    let screen_id: LocalId = 20;
    let content_id: LocalId = 21;
    m.init
        .push(let_widget(screen_id, "screen", nmc("ScrollView", vec![])));
    m.init.push(let_widget(
        content_id,
        "content",
        nmc(
            "VStack",
            vec![Expr::Array(vec![nmc(
                "Text",
                vec![Expr::String("hello".into())],
            )])],
        ),
    ));
    m.init.push(mutator_stmt(
        "scrollviewSetChild",
        vec![Expr::LocalGet(screen_id), Expr::LocalGet(content_id)],
    ));
    m.init.push(app_with_body(Expr::LocalGet(screen_id)));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(
        r.ets_source.contains("Scroll() {"),
        "expected Scroll wrapper:\n{}",
        r.ets_source
    );
    // Child content is rendered inside the inner Column.
    assert!(
        r.ets_source.contains("Text('hello')"),
        "missing scroll child content:\n{}",
        r.ets_source
    );
}

#[test]
fn issue_408_set_padding_emits_modifier_chain() {
    // const card = VStack([]);
    // setPadding(card, 8, 12, 8, 12);
    // setCornerRadius(card, 16);
    // widgetSetBackgroundColor(card, 0.2, 0.5, 0.95, 1);
    // App({body: card});
    let mut m = empty_module();
    let card_id: LocalId = 30;
    m.init.push(let_widget(
        card_id,
        "card",
        nmc("VStack", vec![Expr::Array(vec![])]),
    ));
    m.init.push(mutator_stmt(
        "setPadding",
        vec![
            Expr::LocalGet(card_id),
            Expr::Number(8.0),
            Expr::Number(12.0),
            Expr::Number(8.0),
            Expr::Number(12.0),
        ],
    ));
    m.init.push(mutator_stmt(
        "setCornerRadius",
        vec![Expr::LocalGet(card_id), Expr::Number(16.0)],
    ));
    m.init.push(mutator_stmt(
        "widgetSetBackgroundColor",
        vec![
            Expr::LocalGet(card_id),
            Expr::Number(0.2),
            Expr::Number(0.5),
            Expr::Number(0.95),
            Expr::Number(1.0),
        ],
    ));
    m.init.push(app_with_body(Expr::LocalGet(card_id)));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(
        r.ets_source
            .contains(".padding({ top: 8, right: 12, bottom: 8, left: 12 })"),
        "expected padding modifier:\n{}",
        r.ets_source
    );
    assert!(
        r.ets_source.contains(".borderRadius(16)"),
        "expected borderRadius:\n{}",
        r.ets_source
    );
    // 0.2*255=51, 0.5*255≈128, 0.95*255≈242
    assert!(
        r.ets_source
            .contains(".backgroundColor('rgba(51, 128, 242, 1)')"),
        "expected rgba background:\n{}",
        r.ets_source
    );
}

#[test]
fn issue_479_widget_set_rich_tooltip_emits_bind_popup_modifier() {
    // const btn = Button("Save");
    // const tip = Text("Press to save now");
    // widgetSetRichTooltip(btn, tip, 500);
    // App({body: btn});
    //
    // Asserts the tooltip lowers to ArkUI's `.bindPopup(false, {
    // message: '...' })` modifier chained off the trigger widget.
    // The hover delay is documented but not honored — ArkUI's
    // popup show-trigger is implicit (long-press / click).
    let mut m = empty_module();
    let btn_id: LocalId = 100;
    let tip_id: LocalId = 101;
    m.init.push(let_widget(
        btn_id,
        "btn",
        nmc("Button", vec![Expr::String("Save".into())]),
    ));
    m.init.push(let_widget(
        tip_id,
        "tip",
        nmc("Text", vec![Expr::String("Press to save now".into())]),
    ));
    m.init.push(mutator_stmt(
        "widgetSetRichTooltip",
        vec![
            Expr::LocalGet(btn_id),
            Expr::LocalGet(tip_id),
            Expr::Number(500.0),
        ],
    ));
    m.init.push(app_with_body(Expr::LocalGet(btn_id)));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(
        r.ets_source
            .contains(".bindPopup(false, { message: 'Press to save now' })"),
        "expected bindPopup modifier:\n{}",
        r.ets_source
    );
}

#[test]
fn issue_479_widget_set_rich_tooltip_with_inline_text_content() {
    // Same as above but the content widget is constructed inline,
    // without an intervening LocalGet binding — exercises the
    // direct-call branch of resolve_tooltip_text.
    let mut m = empty_module();
    let btn_id: LocalId = 110;
    m.init.push(let_widget(
        btn_id,
        "btn",
        nmc("Button", vec![Expr::String("Save".into())]),
    ));
    m.init.push(mutator_stmt(
        "widgetSetRichTooltip",
        vec![
            Expr::LocalGet(btn_id),
            nmc("Text", vec![Expr::String("inline tip".into())]),
            Expr::Number(0.0),
        ],
    ));
    m.init.push(app_with_body(Expr::LocalGet(btn_id)));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(
        r.ets_source
            .contains(".bindPopup(false, { message: 'inline tip' })"),
        "expected bindPopup modifier:\n{}",
        r.ets_source
    );
}

#[test]
fn issue_408_conditional_widget_add_child_emits_if_else() {
    // const screen = VStack([]);
    // const btn_phone = Button("phone");
    // const btn_desktop = Button("desktop");
    // if (props.isMobile) { widgetAddChild(screen, btn_phone); }
    // else { widgetAddChild(screen, btn_desktop); }
    // App({body: screen});
    //
    // The condition uses a PropertyGet, which can't be statically
    // folded by the #413 evaluator (only literal-leaf expressions
    // fold). The harvest emits a real `if (...) { ... } else { ... }`
    // block in the ArkTS source.
    let mut m = empty_module();
    let screen_id: LocalId = 40;
    let phone_id: LocalId = 41;
    let desktop_id: LocalId = 42;
    m.init.push(let_widget(
        screen_id,
        "screen",
        nmc("VStack", vec![Expr::Array(vec![])]),
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
    // v0.5.490: dead-branch elim now fires when the condition isn't
    // cleanly serializable. The original PropertyGet(LocalGet(9999),
    // "isMobile") shape would have rendered both branches under
    // `if (true) { ... } else { ... }` — but the else-branch is
    // dead source-wise and Mango exposed this as the "+ New
    // Connection" duplicate-content bug. New behavior: walk only
    // the then-branch when the condition can't be serialized
    // (matches the then-branch heuristic from v0.5.487's
    // Expr::Conditional emit_widget arm).
    m.init.push(Stmt::If {
        condition: Expr::PropertyGet {
            byte_offset: 0,
            object: Box::new(Expr::LocalGet(9999)),
            property: "isMobile".to_string(),
        },
        then_branch: vec![mutator_stmt(
            "widgetAddChild",
            vec![Expr::LocalGet(screen_id), Expr::LocalGet(phone_id)],
        )],
        else_branch: Some(vec![mutator_stmt(
            "widgetAddChild",
            vec![Expr::LocalGet(screen_id), Expr::LocalGet(desktop_id)],
        )]),
    });
    m.init.push(app_with_body(Expr::LocalGet(screen_id)));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    // Then-branch is the only one emitted (heuristic-pick).
    assert!(
        r.ets_source.contains("Button('phone')"),
        "expected then-branch (`Button('phone')`) emitted:\n{}",
        r.ets_source
    );
    // Else-branch is dropped — no `Button('desktop')`.
    assert!(
        !r.ets_source.contains("Button('desktop')"),
        "else-branch must be dropped (cleanly-serializable gate fired):\n{}",
        r.ets_source
    );
}

#[test]
fn issue_408_widget_clear_children_drops_earlier_addchild() {
    // const stack = HStack(0, []);
    // widgetAddChild(stack, btn_a);
    // widgetClearChildren(stack);
    // widgetAddChild(stack, btn_b);
    // App({body: stack}); — only btn_b should render.
    let mut m = empty_module();
    let stack_id: LocalId = 50;
    let a_id: LocalId = 51;
    let b_id: LocalId = 52;
    m.init.push(let_widget(
        stack_id,
        "stack",
        nmc("HStack", vec![Expr::Number(0.0), Expr::Array(vec![])]),
    ));
    m.init.push(let_widget(
        a_id,
        "btn_a",
        nmc("Button", vec![Expr::String("dropped".into())]),
    ));
    m.init.push(let_widget(
        b_id,
        "btn_b",
        nmc("Button", vec![Expr::String("kept".into())]),
    ));
    m.init.push(mutator_stmt(
        "widgetAddChild",
        vec![Expr::LocalGet(stack_id), Expr::LocalGet(a_id)],
    ));
    m.init.push(mutator_stmt(
        "widgetClearChildren",
        vec![Expr::LocalGet(stack_id)],
    ));
    m.init.push(mutator_stmt(
        "widgetAddChild",
        vec![Expr::LocalGet(stack_id), Expr::LocalGet(b_id)],
    ));
    m.init.push(app_with_body(Expr::LocalGet(stack_id)));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(
        !r.ets_source.contains("Button('dropped')"),
        "Button('dropped') should have been cleared:\n{}",
        r.ets_source
    );
    assert!(
        r.ets_source.contains("Button('kept')"),
        "Button('kept') should remain:\n{}",
        r.ets_source
    );
}

#[test]
fn issue_408_untraceable_parent_falls_back_without_crashing() {
    // widgetAddChild(<some unbound expression>, btn) — parent isn't
    // a LocalGet, so the mutation is dropped silently. The page still
    // emits cleanly.
    let mut m = empty_module();
    let stack_id: LocalId = 60;
    m.init.push(let_widget(
        stack_id,
        "stack",
        nmc("VStack", vec![Expr::Array(vec![])]),
    ));
    m.init.push(mutator_stmt(
        "widgetAddChild",
        vec![
            // First arg is NOT a LocalGet — typical "transient widget"
            // shape that the harvest can't statically trace. Should
            // not crash; should be silently skipped.
            nmc("Button", vec![Expr::String("orphan".into())]),
            nmc("Button", vec![Expr::String("child".into())]),
        ],
    ));
    m.init.push(app_with_body(Expr::LocalGet(stack_id)));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    // Stack still renders; mutation silently skipped.
    assert!(
        r.ets_source.contains("Column({ space: 8 })"),
        "stack still renders:\n{}",
        r.ets_source
    );
    // The orphan child shouldn't appear since the mutation didn't
    // resolve to a known parent.
    assert!(
        !r.ets_source.contains("Button('child')"),
        "untraceable child should not have been added:\n{}",
        r.ets_source
    );
}

#[test]
fn issue_408_widget_set_hidden_emits_visibility_modifier() {
    let mut m = empty_module();
    let id: LocalId = 70;
    m.init.push(let_widget(
        id,
        "w",
        nmc("VStack", vec![Expr::Array(vec![])]),
    ));
    m.init.push(mutator_stmt(
        "widgetSetHidden",
        vec![Expr::LocalGet(id), Expr::Number(1.0)],
    ));
    m.init.push(app_with_body(Expr::LocalGet(id)));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(
        r.ets_source.contains(".visibility(Visibility.Hidden)"),
        "missing hidden modifier:\n{}",
        r.ets_source
    );
}

/// Phase 2 v3.5 — `widgetSetHidden` from a Button onClick closure
/// triggers a `@State hidden_<id>` binding + `.visibility(...)` bound
/// modifier. Mango's "+ New Connection" tap pattern.
#[test]
fn phase2_v35_widget_set_hidden_in_closure_emits_state_binding() {
    let mut m = empty_module();
    let target_id: LocalId = 100;
    // const formContainer = VStack(0, []);
    m.init.push(let_widget(
        target_id,
        "formContainer",
        nmc("VStack", vec![Expr::Number(0.0), Expr::Array(vec![])]),
    ));
    // widgetSetHidden(formContainer, 1);  // module-init initial = hidden
    m.init.push(mutator_stmt(
        "widgetSetHidden",
        vec![Expr::LocalGet(target_id), Expr::Number(1.0)],
    ));
    // App({body: VStack(0, [Button("Open", () => widgetSetHidden(formContainer, 0)),
    //                       formContainer])})
    let body_id: LocalId = 101;
    let onclick = Expr::Closure {
        func_id: 0,
        params: vec![],
        return_type: perry_types::Type::Any,
        body: vec![mutator_stmt(
            "widgetSetHidden",
            vec![Expr::LocalGet(target_id), Expr::Number(0.0)],
        )],
        captures: vec![],
        mutable_captures: vec![],
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: false,
        is_async: false,
        is_generator: false,
        is_strict: false,
    };
    m.init.push(let_widget(
        body_id,
        "rootBody",
        nmc(
            "VStack",
            vec![
                Expr::Number(0.0),
                Expr::Array(vec![
                    nmc("Button", vec![Expr::String("Open".to_string()), onclick]),
                    Expr::LocalGet(target_id),
                ]),
            ],
        ),
    ));
    m.init.push(app_with_body(Expr::LocalGet(body_id)));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    // @State decl emitted with module-init initial value (hidden=true).
    assert!(
        r.ets_source
            .contains("@State hidden_vis_0: boolean = true;"),
        "missing @State hidden_vis_0 decl:\n{}",
        r.ets_source
    );
    // applyVisibilityUpdate switch arm.
    assert!(
        r.ets_source
            .contains("case 'vis_0': this.hidden_vis_0 = hidden; break;"),
        "missing applyVisibilityUpdate arm for vis_0:\n{}",
        r.ets_source
    );
    // Bound modifier on the widget itself.
    assert!(
        r.ets_source
            .contains(".visibility(this.hidden_vis_0 ? Visibility.Hidden : Visibility.Visible)"),
        "missing bound .visibility modifier:\n{}",
        r.ets_source
    );
    // No static .visibility(Visibility.Hidden) — that path is replaced
    // by the binding when binding is in effect.
    assert!(
        !r.ets_source.contains(".visibility(Visibility.Hidden)"),
        "static visibility modifier should be replaced by binding:\n{}",
        r.ets_source
    );
    // Drain pump for the visibility queue lives in the onClick body.
    assert!(
        r.ets_source.contains("perryEntry.drainVisibilityUpdate"),
        "missing drainVisibilityUpdate in onClick:\n{}",
        r.ets_source
    );
    // Closure-time call rewritten to setVisibility.
    // (Indirectly verified by its absence as a static `widgetSetHidden`
    // call inside the closure body in the harvested HIR — the rewrite
    // happened in-place. We check the registered closure has had its
    // body modified by inspecting the harvest result's callbacks.)
    assert_eq!(r.callbacks.len(), 1, "expected one harvested closure");
    let cb = &r.callbacks[0];
    if let Expr::Closure { body, .. } = cb {
        // The rewritten closure body should contain a setVisibility
        // NativeMethodCall on perry/arkts (not the original
        // widgetSetHidden on perry/ui).
        let stmt0 = &body[0];
        if let Stmt::Expr(Expr::NativeMethodCall { module, method, .. }) = stmt0 {
            assert_eq!(module, "perry/arkts", "module not rewritten:\n{:?}", stmt0);
            assert_eq!(
                method, "setVisibility",
                "method not rewritten:\n{:?}",
                stmt0
            );
        } else {
            panic!("closure body[0] not a NativeMethodCall: {:?}", stmt0);
        }
    } else {
        panic!("callback[0] not a Closure: {:?}", cb);
    }
}

#[test]
fn issue_408_match_parent_size_emits_100pct_modifiers() {
    let mut m = empty_module();
    let id: LocalId = 80;
    m.init.push(let_widget(
        id,
        "w",
        nmc("VStack", vec![Expr::Array(vec![])]),
    ));
    m.init.push(mutator_stmt(
        "widgetMatchParentWidth",
        vec![Expr::LocalGet(id)],
    ));
    m.init.push(mutator_stmt(
        "widgetMatchParentHeight",
        vec![Expr::LocalGet(id)],
    ));
    m.init.push(app_with_body(Expr::LocalGet(id)));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(
        r.ets_source.contains(".width('100%')"),
        "missing width 100%:\n{}",
        r.ets_source
    );
    assert!(
        r.ets_source.contains(".height('100%')"),
        "missing height 100%:\n{}",
        r.ets_source
    );
}

#[test]
fn issue_408_stack_distribution_and_alignment_emit_flexalign_modifiers() {
    // Uses HStack, so post-#413 the alignment enum is VerticalAlign
    // (Row's cross-axis is vertical). Pre-#413 this test asserted
    // HorizontalAlign.Center — which ArkTS strict-mode rejected at
    // assembleHap with "type 'HorizontalAlign' not assignable to
    // 'VerticalAlign'".
    let mut m = empty_module();
    let id: LocalId = 90;
    m.init.push(let_widget(
        id,
        "w",
        nmc("HStack", vec![Expr::Number(0.0), Expr::Array(vec![])]),
    ));
    m.init.push(mutator_stmt(
        "stackSetDistribution",
        vec![Expr::LocalGet(id), Expr::Number(3.0)], // SpaceBetween
    ));
    m.init.push(mutator_stmt(
        "stackSetAlignment",
        vec![Expr::LocalGet(id), Expr::Number(1.0)], // Center
    ));
    m.init.push(app_with_body(Expr::LocalGet(id)));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(
        r.ets_source
            .contains(".justifyContent(FlexAlign.SpaceBetween)"),
        "missing distribution modifier:\n{}",
        r.ets_source
    );
    assert!(
        r.ets_source.contains(".alignItems(VerticalAlign.Center)"),
        "missing alignment modifier (HStack should pick VerticalAlign):\n{}",
        r.ets_source
    );
    // Negative-pin: must NOT emit HorizontalAlign for HStack.
    assert!(
        !r.ets_source.contains("HorizontalAlign"),
        "HStack must not emit HorizontalAlign:\n{}",
        r.ets_source
    );
}

#[test]
fn text_styling_mutators_emit_arkui_modifiers() {
    // #408 follow-up — `textSetFontSize` / `textSetColor` /
    // `textSetFontWeight` / `textSetFontFamily` had been falling
    // through to the unrecognized-mutator path, producing
    // `// not yet handled` comments instead of real ArkUI modifiers.
    // Mango uses these heavily for branded title styling — without
    // them the toolbar shows up as plain default-styled text.
    let mut m = empty_module();
    let id: LocalId = 50;
    m.init.push(let_widget(
        id,
        "title",
        nmc("Text", vec![Expr::String("Mango".into())]),
    ));
    m.init.push(mutator_stmt(
        "textSetFontSize",
        vec![Expr::LocalGet(id), Expr::Number(28.0)],
    ));
    m.init.push(mutator_stmt(
        "textSetFontWeight",
        // (widget, size, weight_scale) — matches Apple's
        // systemFont(ofSize: weight:) signature. weight_scale 0..1
        // maps to ArkUI's 100..900 (rounded to nearest 100). 1.0
        // → 900 (Bold-equivalent).
        vec![Expr::LocalGet(id), Expr::Number(28.0), Expr::Number(1.0)],
    ));
    m.init.push(mutator_stmt(
        "textSetFontFamily",
        vec![Expr::LocalGet(id), Expr::String("Inter".into())],
    ));
    m.init.push(mutator_stmt(
        "textSetColor",
        vec![
            Expr::LocalGet(id),
            Expr::Number(0.5),
            Expr::Number(0.25),
            Expr::Number(0.0),
            Expr::Number(1.0),
        ],
    ));
    m.init.push(app_with_body(Expr::LocalGet(id)));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    for must in [
        ".fontSize(28)",
        ".fontWeight(900)",
        ".fontFamily('Inter')",
        ".fontColor('rgba(128, 64, 0, 1)')",
    ] {
        assert!(
            r.ets_source.contains(must),
            "missing {must} in:\n{}",
            r.ets_source
        );
    }
    // Negative-pin: must NOT be in the unrecognized-mutator branch.
    assert!(
        !r.ets_source.contains("textSetFontSize` not yet handled"),
        "textSetFontSize should be handled, not flagged:\n{}",
        r.ets_source
    );
}

#[test]
fn unrecognized_mutator_comment_does_not_swallow_following_modifier() {
    // #408 follow-up — `Mutation::Comment` previously emitted as
    // `\n// X`, which is a line comment runs to EOL. Modifier
    // mutations chain on the same physical line in the emitted
    // ArkTS (e.g. `}.padding(...).visibility(...)`); a `\n// X`
    // splice between two modifiers caused the second modifier to
    // be eaten by the comment:
    //   `}.padding(...)\n// X.visibility(...)`
    // ArkTS parses `// X.visibility(...)` as one comment line and
    // the `.visibility` modifier silently disappears. Fix: emit
    // unrecognized-mutator diagnostics as inline `/* X */` block
    // comments instead.
    let mut m = empty_module();
    let id: LocalId = 60;
    m.init.push(let_widget(
        id,
        "label",
        nmc("Text", vec![Expr::String("hi".into())]),
    ));
    // Sandwich an unrecognized mutator between two recognized ones
    // so we exercise the "comment between modifiers" shape.
    m.init.push(mutator_stmt(
        "textSetFontSize",
        vec![Expr::LocalGet(id), Expr::Number(20.0)],
    ));
    m.init.push(mutator_stmt(
        "totallyMadeUpMutator",
        vec![Expr::LocalGet(id), Expr::Number(99.0)],
    ));
    m.init.push(mutator_stmt(
        "widgetSetHidden",
        vec![Expr::LocalGet(id), Expr::Number(1.0)],
    ));
    m.init.push(app_with_body(Expr::LocalGet(id)));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    // Both modifiers AROUND the unrecognized one must be present
    // and not swallowed.
    assert!(
        r.ets_source.contains(".fontSize(20)"),
        "fontSize should be present:\n{}",
        r.ets_source
    );
    assert!(
        r.ets_source.contains(".visibility(Visibility.Hidden)"),
        "visibility should be present after the comment:\n{}",
        r.ets_source
    );
    // The comment itself must use inline block-comment shape.
    assert!(
        r.ets_source
            .contains("/* perry/ui mutator `totallyMadeUpMutator`"),
        "comment should be inline /* */, not //:\n{}",
        r.ets_source
    );
    // Negative-pin: no `\n// ` patterns in the modifier section
    // (which would re-introduce the swallow bug).
    assert!(
        !r.ets_source.contains("\n// perry/ui mutator"),
        "comments must not be line comments anymore:\n{}",
        r.ets_source
    );
}

#[test]
fn stack_alignment_value_names_match_axis_enum() {
    // #413 follow-up — `VerticalAlign` doesn't have `Start`/`End`
    // (those exist only on `HorizontalAlign`). It uses `Top`/`Bottom`.
    // Picking `VerticalAlign.Start` produces an ArkTS strict-mode
    // error: "Property 'Start' does not exist on type 'typeof
    // VerticalAlign'". Mango hit this on the browserContent HStack
    // with stackSetAlignment(0) (= start semantics).
    //
    // Same semantic input value (0=start, 1=center, 2=end) must map
    // to axis-correct value-names — Top/Bottom for VerticalAlign,
    // Start/End for HorizontalAlign.
    for (ctor, n_in, expected_modifier) in [
        ("HStack", 0.0, ".alignItems(VerticalAlign.Top)"),
        ("HStack", 1.0, ".alignItems(VerticalAlign.Center)"),
        ("HStack", 2.0, ".alignItems(VerticalAlign.Bottom)"),
        ("VStack", 0.0, ".alignItems(HorizontalAlign.Start)"),
        ("VStack", 1.0, ".alignItems(HorizontalAlign.Center)"),
        ("VStack", 2.0, ".alignItems(HorizontalAlign.End)"),
    ] {
        let mut m = empty_module();
        let id: LocalId = 90;
        m.init.push(let_widget(
            id,
            "w",
            nmc(ctor, vec![Expr::Number(0.0), Expr::Array(vec![])]),
        ));
        m.init.push(mutator_stmt(
            "stackSetAlignment",
            vec![Expr::LocalGet(id), Expr::Number(n_in)],
        ));
        m.init.push(app_with_body(Expr::LocalGet(id)));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(
            r.ets_source.contains(expected_modifier),
            "{ctor} stackSetAlignment({n_in}) should emit '{expected_modifier}':\n{src}",
            src = r.ets_source
        );
    }
}

#[test]
fn issue_408_mango_three_screen_shape_renders_all_screens() {
    // Composite test mirroring the Mango shape from #408 — three
    // top-level screens built procedurally with widgetAddChild +
    // styling mutators, all wrapped in a single VStack.
    let mut m = empty_module();
    let root_id: LocalId = 100;
    let conn_id: LocalId = 101;
    let browser_id: LocalId = 102;
    let info_id: LocalId = 103;
    let conn_btn: LocalId = 110;
    let browser_btn: LocalId = 111;
    let info_btn: LocalId = 112;
    m.init.push(let_widget(
        root_id,
        "root",
        nmc("VStack", vec![Expr::Array(vec![])]),
    ));
    // Three screen containers
    m.init.push(let_widget(
        conn_id,
        "connectionScreen",
        nmc("VStack", vec![Expr::Array(vec![])]),
    ));
    m.init.push(let_widget(
        browser_id,
        "browserScreen",
        nmc("ScrollView", vec![]),
    ));
    m.init.push(let_widget(
        info_id,
        "infoScreen",
        nmc("HStack", vec![Expr::Number(8.0), Expr::Array(vec![])]),
    ));
    // Widget-level child buttons
    m.init.push(let_widget(
        conn_btn,
        "conn_btn",
        nmc("Button", vec![Expr::String("Connect".into())]),
    ));
    m.init.push(let_widget(
        browser_btn,
        "browser_btn",
        nmc("Button", vec![Expr::String("Browse".into())]),
    ));
    m.init.push(let_widget(
        info_btn,
        "info_btn",
        nmc("Button", vec![Expr::String("Info".into())]),
    ));
    // widgetAddChild calls — connection screen gets a button
    m.init.push(mutator_stmt(
        "widgetAddChild",
        vec![Expr::LocalGet(conn_id), Expr::LocalGet(conn_btn)],
    ));
    // browserScreen uses scrollviewSetChild + a wrapper VStack
    let browser_content_id: LocalId = 120;
    m.init.push(let_widget(
        browser_content_id,
        "browser_content",
        nmc(
            "VStack",
            vec![Expr::Array(vec![Expr::LocalGet(browser_btn)])],
        ),
    ));
    m.init.push(mutator_stmt(
        "scrollviewSetChild",
        vec![
            Expr::LocalGet(browser_id),
            Expr::LocalGet(browser_content_id),
        ],
    ));
    m.init.push(mutator_stmt(
        "widgetAddChild",
        vec![Expr::LocalGet(info_id), Expr::LocalGet(info_btn)],
    ));
    // Style the root
    m.init.push(mutator_stmt(
        "setPadding",
        vec![
            Expr::LocalGet(root_id),
            Expr::Number(16.0),
            Expr::Number(16.0),
            Expr::Number(16.0),
            Expr::Number(16.0),
        ],
    ));
    // Add screens to root
    m.init.push(mutator_stmt(
        "widgetAddChild",
        vec![Expr::LocalGet(root_id), Expr::LocalGet(conn_id)],
    ));
    m.init.push(mutator_stmt(
        "widgetAddChild",
        vec![Expr::LocalGet(root_id), Expr::LocalGet(browser_id)],
    ));
    m.init.push(mutator_stmt(
        "widgetAddChild",
        vec![Expr::LocalGet(root_id), Expr::LocalGet(info_id)],
    ));
    m.init.push(app_with_body(Expr::LocalGet(root_id)));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    // All three screens' contents must surface.
    assert!(
        r.ets_source.contains("Button('Connect')"),
        "missing Connect:\n{}",
        r.ets_source
    );
    assert!(
        r.ets_source.contains("Button('Browse')"),
        "missing Browse:\n{}",
        r.ets_source
    );
    assert!(
        r.ets_source.contains("Button('Info')"),
        "missing Info:\n{}",
        r.ets_source
    );
    assert!(
        r.ets_source
            .contains(".padding({ top: 16, right: 16, bottom: 16, left: 16 })"),
        "missing root padding:\n{}",
        r.ets_source
    );
    assert!(
        r.ets_source.contains("Scroll() {"),
        "missing browser scroll:\n{}",
        r.ets_source
    );
}
