// Basic widget emission tests: Text/VStack/HStack/Button/TextField/
// Toggle/Slider/Divider, reactive Text ids, animation/shadow/decoration/
// image, inline-style objects, and ForEach lowering.
use super::*;

#[test]
fn emits_none_for_empty_module() {
    let mut m = empty_module();
    assert!(emit_index_ets(&mut m).unwrap().is_none());
}

#[test]
fn text_strips_app_call() {
    let mut m = empty_module();
    m.init
        .push(app_with_body(nmc("Text", vec![Expr::String("hi".into())])));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains("Text('hi').fontSize(20)"));
    assert!(matches!(m.init[0], Stmt::Expr(Expr::Number(_))));
    assert_eq!(r.callbacks.len(), 0);
}

#[test]
fn vstack_with_text_children() {
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "VStack",
        vec![Expr::Array(vec![
            nmc("Text", vec![Expr::String("a".into())]),
            nmc("Text", vec![Expr::String("b".into())]),
        ])],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains("Column({ space: 8 })"));
    assert!(r.ets_source.contains("Text('a').fontSize(20)"));
    assert!(r.ets_source.contains("Text('b').fontSize(20)"));
}

#[test]
fn vstack_with_explicit_spacing() {
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "VStack",
        vec![
            Expr::Number(16.0),
            Expr::Array(vec![nmc("Text", vec![Expr::String("a".into())])]),
        ],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains("Column({ space: 16 })"));
}

#[test]
fn hstack_emits_row() {
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "HStack",
        vec![Expr::Array(vec![nmc("Spacer", vec![])])],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains("Row({ space: 8 })"));
    assert!(r.ets_source.contains("Blank()"));
}

#[test]
fn button_label_only_no_closure_drops_onclick() {
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "Button",
        vec![
            Expr::String("Save".into()),
            Expr::Number(0.0), // not a closure — placeholder
        ],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains("Button('Save').fontSize(16)"));
    assert!(!r.ets_source.contains(".onClick"));
    assert_eq!(r.callbacks.len(), 0);
}

#[test]
fn button_with_closure_emits_onclick_and_captures_callback() {
    // Phase 2 v2 + v3 headline test: Button("Save", () => {}) emits
    // an onClick that invokes the registered closure THEN drains the
    // toast queue (so `showToast(msg)` calls inside the closure body
    // produce visible popups).
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "Button",
        vec![Expr::String("Save".into()), closure_stub()],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    // v2: invokeCallback dispatches the registered closure.
    assert!(r.ets_source.contains("perryEntry.invokeCallback(0)"));
    // v3: drain loop dispatches queued toasts after the closure
    // returns. Single-line search avoids depending on whitespace.
    assert!(r.ets_source.contains("perryEntry.drainToast()"));
    assert!(r.ets_source.contains("promptAction.showToast"));
    assert_eq!(r.callbacks.len(), 1);
    assert!(matches!(r.callbacks[0], Expr::Closure { .. }));
    // Page wrapper imports both perryEntry and promptAction so the
    // auto-emitted onClick body resolves at ArkTS compile time.
    assert!(r
        .ets_source
        .contains("import perryEntry from 'libentry.so'"));
    assert!(r
        .ets_source
        .contains("import promptAction from '@ohos.promptAction'"));
}

#[test]
fn multi_button_assigns_sequential_callback_slots() {
    // Two buttons in a VStack — slot 0 and slot 1 in declaration order.
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "VStack",
        vec![Expr::Array(vec![
            nmc("Button", vec![Expr::String("First".into()), closure_stub()]),
            nmc(
                "Button",
                vec![Expr::String("Second".into()), closure_stub()],
            ),
        ])],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains("perryEntry.invokeCallback(0)"));
    assert!(r.ets_source.contains("perryEntry.invokeCallback(1)"));
    assert_eq!(r.callbacks.len(), 2);
}

#[test]
fn textfield_placeholder() {
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "TextField",
        vec![Expr::String("Search…".into()), Expr::Number(0.0)],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r
        .ets_source
        .contains("TextInput({ placeholder: 'Search…' })"));
}

#[test]
fn toggle_with_label_emits_row() {
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "Toggle",
        vec![Expr::String("Notifications".into()), Expr::Number(0.0)],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains("Row({ space: 8 })"));
    assert!(r.ets_source.contains("Text('Notifications')"));
    assert!(r
        .ets_source
        .contains("Toggle({ type: ToggleType.Switch, isOn: false })"));
}

#[test]
fn slider_min_max() {
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "Slider",
        vec![
            Expr::Number(0.0),
            Expr::Number(100.0),
            Expr::Number(0.0), // would be closure
        ],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains("min: 0"));
    assert!(r.ets_source.contains("max: 100"));
}

#[test]
fn divider_no_args() {
    let mut m = empty_module();
    m.init.push(app_with_body(nmc("Divider", vec![])));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains("Divider()"));
}

#[test]
fn nested_vstack_in_hstack() {
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "VStack",
        vec![Expr::Array(vec![nmc(
            "HStack",
            vec![Expr::Array(vec![
                nmc("Text", vec![Expr::String("L".into())]),
                nmc("Text", vec![Expr::String("R".into())]),
            ])],
        )])],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains("Column({ space: 8 })"));
    assert!(r.ets_source.contains("Row({ space: 8 })"));
    assert!(r.ets_source.contains("Text('L')"));
    assert!(r.ets_source.contains("Text('R')"));
}

#[test]
fn local_get_escape_follows_const_binding() {
    let mut m = empty_module();
    // Simulate: const t = Text("via let"); App({body: t});
    m.init.push(Stmt::Let {
        id: 7,
        name: "t".to_string(),
        ty: perry_hir::types::Type::Any,
        mutable: false,
        init: Some(nmc("Text", vec![Expr::String("via let".into())])),
    });
    m.init.push(app_with_body(Expr::LocalGet(7)));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains("Text('via let')"));
}

#[test]
fn text_with_id_registers_reactive_slot() {
    // Phase 2 v3 Option 2: Text("Count: 0", "counter") must:
    //   - emit @State text_counter: string = 'Count: 0' on the page
    //   - emit Text(this.text_counter) at the widget site
    //   - register a switch arm in applyTextUpdate
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "Text",
        vec![
            Expr::String("Count: 0".into()),
            Expr::String("counter".into()),
        ],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r
        .ets_source
        .contains("@State text_counter: string = 'Count: 0'"));
    assert!(r.ets_source.contains("Text(this.text_counter)"));
    assert!(r
        .ets_source
        .contains("case 'counter': this.text_counter = value; break;"));
}

#[test]
fn text_id_sanitization_drops_invalid_chars() {
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "Text",
        vec![
            Expr::String("hi".into()),
            Expr::String("user-name".into()), // hyphen → underscore
        ],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains("@State text_user_name"));
    assert!(r.ets_source.contains("case 'user-name'"));
}

#[test]
fn toggle_with_closure_emits_onchange_with_invokecallback1() {
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "Toggle",
        vec![Expr::String("Notify".into()), closure_stub()],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains(".onChange((isOn: boolean) => {"));
    assert!(r.ets_source.contains("perryEntry.invokeCallback1(0, isOn)"));
    assert_eq!(r.callbacks.len(), 1);
}

#[test]
fn textfield_with_closure_forwards_value_to_invokecallback1() {
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "TextField",
        vec![Expr::String("Search…".into()), closure_stub()],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains(".onChange((value: string) => {"));
    assert!(r
        .ets_source
        .contains("perryEntry.invokeCallback1(0, value)"));
}

#[test]
fn slider_with_closure_forwards_value_to_invokecallback1() {
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "Slider",
        vec![Expr::Number(0.0), Expr::Number(100.0), closure_stub()],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r
        .ets_source
        .contains(".onChange((value: number, _mode: SliderChangeMode) => {"));
    assert!(r
        .ets_source
        .contains("perryEntry.invokeCallback1(0, value)"));
}

#[test]
fn button_onclick_drains_both_toast_and_text_update_queues() {
    // The generated onClick body should drain BOTH queues so a
    // closure that calls showToast AND setText sees both effects.
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "Button",
        vec![Expr::String("Tap".into()), closure_stub()],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains("perryEntry.drainToast()"));
    assert!(r.ets_source.contains("perryEntry.drainTextUpdate()"));
    assert!(r
        .ets_source
        .contains("this.applyTextUpdate(__u.id, __u.value)"));
}

// ----- Phase 2 v13: animation / shadow / textDecoration / image asset -----

#[test]
fn animation_modifier_maps_curve_string_to_curve_enum() {
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "Text",
        vec![
            Expr::String("hi".into()),
            Expr::Object(vec![(
                "animation".into(),
                Expr::Object(vec![
                    ("duration".into(), Expr::Number(300.0)),
                    ("curve".into(), Expr::String("ease-in".into())),
                ]),
            )]),
        ],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r
        .ets_source
        .contains(".animation({ duration: 300, curve: Curve.EaseIn })"));
}

#[test]
fn shadow_modifier_maps_blur_to_radius_offsets_to_offsetXY() {
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "Text",
        vec![
            Expr::String("hi".into()),
            Expr::Object(vec![(
                "shadow".into(),
                Expr::Object(vec![
                    ("color".into(), Expr::String("black".into())),
                    ("blur".into(), Expr::Number(8.0)),
                    ("offsetX".into(), Expr::Number(2.0)),
                    ("offsetY".into(), Expr::Number(4.0)),
                ]),
            )]),
        ],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    // ArkUI's shadow uses `radius` not `blur`; offsetX/Y match.
    assert!(r.ets_source.contains(".shadow({"));
    assert!(r.ets_source.contains("color: 'black'"));
    assert!(r.ets_source.contains("radius: 8"));
    assert!(r.ets_source.contains("offsetX: 2"));
    assert!(r.ets_source.contains("offsetY: 4"));
}

#[test]
fn text_decoration_underline_maps_to_decoration_modifier() {
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "Text",
        vec![
            Expr::String("hi".into()),
            Expr::Object(vec![(
                "textDecoration".into(),
                Expr::String("underline".into()),
            )]),
        ],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r
        .ets_source
        .contains(".decoration({ type: TextDecorationType.Underline })"));
}

#[test]
fn text_decoration_strikethrough_maps_to_linethrough() {
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "Text",
        vec![
            Expr::String("hi".into()),
            Expr::Object(vec![(
                "textDecoration".into(),
                Expr::String("strikethrough".into()),
            )]),
        ],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r
        .ets_source
        .contains(".decoration({ type: TextDecorationType.LineThrough })"));
}

#[test]
fn image_app_media_path_maps_to_resource_accessor() {
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "Image",
        vec![Expr::String("@app.media/icon".into())],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    // `$r('app.media.icon')` (no quotes around the $r() arg).
    assert!(r.ets_source.contains("Image($r('app.media.icon'))"));
    // Plain string passthrough still works for HTTP URLs etc.
    assert!(!r.ets_source.contains("'@app.media/icon'"));
}

#[test]
fn image_plain_url_passes_through_as_string() {
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "Image",
        vec![Expr::String("https://example.com/foo.png".into())],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r
        .ets_source
        .contains("Image('https://example.com/foo.png')"));
}

// ----- Phase 2 v5: inline style + ForEach -----

#[test]
fn inline_style_object_emits_arkui_modifier_chain() {
    // Button("Save", () => {}, { backgroundColor: "blue", borderRadius: 8, opacity: 0.9 })
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "Button",
        vec![
            Expr::String("Save".into()),
            closure_stub(),
            Expr::Object(vec![
                ("backgroundColor".into(), Expr::String("blue".into())),
                ("borderRadius".into(), Expr::Number(8.0)),
                ("opacity".into(), Expr::Number(0.9)),
            ]),
        ],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains(".backgroundColor('blue')"));
    assert!(r.ets_source.contains(".borderRadius(8)"));
    assert!(r.ets_source.contains(".opacity(0.9)"));
}

#[test]
fn inline_style_color_object_emits_rgba() {
    // Text("hi", { color: { r: 0.2, g: 0.5, b: 0.95, a: 1 } })
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "Text",
        vec![
            Expr::String("hi".into()),
            Expr::Object(vec![(
                "color".into(),
                Expr::Object(vec![
                    ("r".into(), Expr::Number(0.2)),
                    ("g".into(), Expr::Number(0.5)),
                    ("b".into(), Expr::Number(0.95)),
                    ("a".into(), Expr::Number(1.0)),
                ]),
            )]),
        ],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    // 0.2 * 255 = 51, 0.5 * 255 ≈ 128, 0.95 * 255 ≈ 242
    assert!(r.ets_source.contains(".fontColor('rgba(51, 128, 242, 1)')"));
}

#[test]
fn inline_style_padding_per_side_object() {
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "Text",
        vec![
            Expr::String("hi".into()),
            Expr::Object(vec![(
                "padding".into(),
                Expr::Object(vec![
                    ("top".into(), Expr::Number(10.0)),
                    ("bottom".into(), Expr::Number(20.0)),
                ]),
            )]),
        ],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains(".padding({ top: 10, bottom: 20 })"));
}

#[test]
fn inline_style_border_combines_color_and_width() {
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "Text",
        vec![
            Expr::String("hi".into()),
            Expr::Object(vec![
                ("borderColor".into(), Expr::String("red".into())),
                ("borderWidth".into(), Expr::Number(2.0)),
            ]),
        ],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    // ArkUI's `.border({ width, color })` is one combined modifier.
    assert!(r.ets_source.contains(".border({ width: 2, color: 'red' })"));
}

#[test]
fn text_with_id_string_is_NOT_treated_as_style() {
    // Text("Count: 0", "counter") — second string arg is the reactive
    // id, NOT a style object. extract_style_object returns None for
    // String args, so the v3.2 reactive path still wins.
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "Text",
        vec![
            Expr::String("Count: 0".into()),
            Expr::String("counter".into()),
        ],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains("Text(this.text_counter)"));
    // Should NOT have any inline-style modifiers tacked on.
    assert!(!r.ets_source.contains(".backgroundColor"));
}

#[test]
fn for_each_lowers_array_map_in_vstack() {
    // VStack(items.map(item => Text(item))) — the closure-param `item`
    // resolves via arkts_locals → __item in the emitted ForEach body.
    let mut m = empty_module();
    // Build `Expr::ArrayMap { array: ["a","b","c"], callback: (p) => Text(p) }`.
    let item_param = perry_hir::ir::Param {
        id: 42,
        name: "item".to_string(),
        ty: perry_hir::types::Type::Any,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    };
    let inner_text = nmc("Text", vec![Expr::LocalGet(42)]);
    let map_expr = Expr::ArrayMap {
        array: Box::new(Expr::Array(vec![
            Expr::String("a".into()),
            Expr::String("b".into()),
            Expr::String("c".into()),
        ])),
        callback: Box::new(Expr::Closure {
            func_id: 0 as perry_hir::types::FuncId,
            params: vec![item_param],
            return_type: perry_hir::types::Type::Any,
            body: vec![Stmt::Return(Some(inner_text))],
            captures: vec![],
            mutable_captures: vec![],
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: false,
            is_async: false,
            is_generator: false,
            is_strict: false,
        }),
    };
    m.init.push(app_with_body(nmc(
        "VStack",
        vec![Expr::Array(vec![map_expr])],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r
        .ets_source
        .contains("ForEach(['a', 'b', 'c'], (__item: any)"));
    // Body resolves `LocalGet(item_param.id)` → __item.
    assert!(r.ets_source.contains("Text(__item)"));
}
