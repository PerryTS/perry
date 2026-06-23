// Test module mechanically split out of lib.rs (issue #1100). Declared
// in lib.rs as `#[cfg(test)] mod tests;` so `use super::*;` keeps
// resolving to the crate root. Pure code move; no logic changes.

use super::*;

fn empty_module() -> Module {
    Module {
        name: "test".to_string(),
        imports: vec![],
        exports: vec![],
        classes: vec![],
        interfaces: vec![],
        type_aliases: vec![],
        enums: vec![],
        globals: vec![],
        functions: vec![],
        script_global_functions: vec![],
        references_global_this: false,
        init: vec![],
        exported_native_instances: vec![],
        exported_func_return_native_instances: vec![],
        exported_objects: vec![],
        exported_functions: vec![],
        widgets: vec![],
        uses_fetch: false,
        uses_webassembly: false,
        init_was_unrolled: false,
        extern_funcs: vec![],
        has_top_level_await: false,
        init_kind: perry_hir::ModuleInitKind::Eager,
        async_step_closures: std::collections::HashSet::new(),
        closure_display_names: std::collections::HashMap::new(),
        closure_source_text: std::collections::HashMap::new(),
        async_generator_funcs: std::collections::HashSet::new(),
        gen_param_prologue_len: std::collections::HashMap::new(),
    }
}

fn nmc(method: &str, args: Vec<Expr>) -> Expr {
    Expr::NativeMethodCall {
        module: "perry/ui".to_string(),
        class_name: None,
        object: None,
        method: method.to_string(),
        args,
    }
}

fn app_with_body(body: Expr) -> Stmt {
    Stmt::Expr(Expr::NativeMethodCall {
        module: "perry/ui".to_string(),
        class_name: None,
        object: None,
        method: "App".to_string(),
        args: vec![Expr::Object(vec![("body".to_string(), body)])],
    })
}

fn closure_stub() -> Expr {
    Expr::Closure {
        func_id: 0 as perry_types::FuncId,
        params: vec![],
        return_type: perry_types::Type::Any,
        body: vec![],
        captures: vec![],
        mutable_captures: vec![],
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: false,
        is_async: false,
        is_generator: false,
        is_strict: false,
    }
}

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
        ty: perry_types::Type::Any,
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
        ty: perry_types::Type::Any,
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
            func_id: 0 as perry_types::FuncId,
            params: vec![item_param],
            return_type: perry_types::Type::Any,
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

#[test]
// ----- Phase 2 v12: Tabs / Modal / Menu / Grid -----
#[test]
fn tabs_emits_tabcontent_per_spec() {
    // Tabs([{label: "Home", body: Text("home content")}, {label: "Settings", body: Text("settings")}])
    let mut m = empty_module();
    let tab1 = Expr::Object(vec![
        ("label".into(), Expr::String("Home".into())),
        (
            "body".into(),
            nmc("Text", vec![Expr::String("home content".into())]),
        ),
    ]);
    let tab2 = Expr::Object(vec![
        ("label".into(), Expr::String("Settings".into())),
        (
            "body".into(),
            nmc("Text", vec![Expr::String("settings".into())]),
        ),
    ]);
    m.init.push(app_with_body(nmc(
        "Tabs",
        vec![Expr::Array(vec![tab1, tab2])],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains("Tabs() {"));
    assert!(r.ets_source.contains(".tabBar('Home')"));
    assert!(r.ets_source.contains(".tabBar('Settings')"));
    assert!(r.ets_source.contains("Text('home content')"));
    assert!(r.ets_source.contains("Text('settings')"));
}

#[test]
fn menu_emits_buttons_per_item() {
    let mut m = empty_module();
    let item1 = Expr::Object(vec![
        ("label".into(), Expr::String("Edit".into())),
        ("action".into(), closure_stub()),
    ]);
    let item2 = Expr::Object(vec![
        ("label".into(), Expr::String("Delete".into())),
        ("action".into(), closure_stub()),
    ]);
    m.init.push(app_with_body(nmc(
        "Menu",
        vec![Expr::Array(vec![item1, item2])],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains("Button('Edit')"));
    assert!(r.ets_source.contains("Button('Delete')"));
    // Both action closures should register (slot 0 + slot 1).
    assert!(r.ets_source.contains("perryEntry.invokeCallback(0)"));
    assert!(r.ets_source.contains("perryEntry.invokeCallback(1)"));
    assert_eq!(r.callbacks.len(), 2);
}

#[test]
fn grid_emits_columns_template_and_griditems() {
    // Grid(3, [Text("a"), Text("b"), Text("c")])
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "Grid",
        vec![
            Expr::Number(3.0),
            Expr::Array(vec![
                nmc("Text", vec![Expr::String("a".into())]),
                nmc("Text", vec![Expr::String("b".into())]),
                nmc("Text", vec![Expr::String("c".into())]),
            ]),
        ],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains("Grid() {"));
    assert!(r.ets_source.contains(".columnsTemplate('1fr 1fr 1fr')"));
    assert!(r.ets_source.contains("GridItem()"));
    assert!(r.ets_source.contains("Text('a')"));
    assert!(r.ets_source.contains("Text('c')"));
}

#[test]
fn modal_emits_placeholder_with_runtime_hint() {
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "Modal",
        vec![Expr::String("Title".into())],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    // Phase 2 v12 emits a placeholder + comment pointing at the
    // showDialog runtime FFI follow-up.
    assert!(r.ets_source.contains("// Modal:"));
    assert!(r.ets_source.contains("showDialog"));
}

// ----- Phase 2 v11: NavStack multi-page navigation -----

#[test]
fn navstack_emits_state_driven_branches() {
    // const route = state("home");
    // App({body: NavStack(route, [
    //     {name: "home", body: Text("Home")},
    //     {name: "detail", body: Text("Detail")},
    // ])});
    let mut m = empty_module();
    m.init.push(Stmt::Let {
        id: 5,
        name: "route".to_string(),
        ty: perry_types::Type::Any,
        mutable: false,
        init: Some(state_call(Expr::String("home".into()))),
    });
    let routes = Expr::Array(vec![
        Expr::Object(vec![
            ("name".into(), Expr::String("home".into())),
            (
                "body".into(),
                nmc("Text", vec![Expr::String("Home".into())]),
            ),
        ]),
        Expr::Object(vec![
            ("name".into(), Expr::String("detail".into())),
            (
                "body".into(),
                nmc("Text", vec![Expr::String("Detail".into())]),
            ),
        ]),
    ]);
    m.init.push(app_with_body(nmc(
        "NavStack",
        vec![Expr::LocalGet(5), routes],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    // Should register an @State decl for the synth id (v6 path).
    assert!(
        r.ets_source.contains("@State text___state_0"),
        "missing v6 @State decl:\n{}",
        r.ets_source
    );
    // First arm is `if`, second is `else if`. The state field used
    // is `this.text___state_0` since the synth id (`__state_0`)
    // sanitizes to `__state_0` and gets prefixed with `text_`.
    assert!(
        r.ets_source.contains("if (this.text___state_0 === 'home')"),
        "missing if-arm for first route:\n{}",
        r.ets_source
    );
    assert!(
        r.ets_source
            .contains("else if (this.text___state_0 === 'detail')"),
        "missing else-if for second route:\n{}",
        r.ets_source
    );
    // Both bodies should be present.
    assert!(r.ets_source.contains("Text('Home')"));
    assert!(r.ets_source.contains("Text('Detail')"));
}

#[test]
fn navstack_no_state_falls_back_to_first_route() {
    // NavStack(<plain non-state local>, [...]) — first arg isn't
    // registered in state_registry, so emit falls back to rendering
    // the first route only with a developer-facing hint comment.
    let mut m = empty_module();
    m.init.push(Stmt::Let {
        id: 7,
        name: "x".to_string(),
        ty: perry_types::Type::Any,
        mutable: false,
        init: Some(Expr::String("home".into())),
    });
    let routes = Expr::Array(vec![Expr::Object(vec![
        ("name".into(), Expr::String("home".into())),
        (
            "body".into(),
            nmc("Text", vec![Expr::String("Home".into())]),
        ),
    ])]);
    m.init.push(app_with_body(nmc(
        "NavStack",
        vec![Expr::LocalGet(7), routes],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    // Hint comment is in the output.
    assert!(
        r.ets_source
            .contains("first arg must be a `state<string>(...)` local"),
        "missing fallback hint:\n{}",
        r.ets_source
    );
    // Body of first route still rendered.
    assert!(r.ets_source.contains("Text('Home')"));
}

#[test]
fn navstack_empty_routes_emits_empty_column_with_comment() {
    let mut m = empty_module();
    m.init.push(Stmt::Let {
        id: 5,
        name: "route".to_string(),
        ty: perry_types::Type::Any,
        mutable: false,
        init: Some(state_call(Expr::String("home".into()))),
    });
    m.init.push(app_with_body(nmc(
        "NavStack",
        vec![Expr::LocalGet(5), Expr::Array(vec![])],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains("// NavStack: empty routes array"));
}

#[test]
fn navstack_set_in_closure_rewrites_to_settext() {
    // const route = state("home");
    // Button("Detail", () => route.set("detail")) — the closure body
    // should rewrite via the existing v6 `state.set(v)` → setText
    // path so navigation actually triggers a re-render.
    let mut m = empty_module();
    m.init.push(Stmt::Let {
        id: 5,
        name: "route".to_string(),
        ty: perry_types::Type::Any,
        mutable: false,
        init: Some(state_call(Expr::String("home".into()))),
    });
    let nav_button = nmc(
        "Button",
        vec![
            Expr::String("Go".into()),
            Expr::Closure {
                func_id: 0 as perry_types::FuncId,
                params: vec![],
                return_type: perry_types::Type::Any,
                body: vec![Stmt::Expr(state_method_call(
                    5,
                    "set",
                    vec![Expr::String("detail".into())],
                ))],
                captures: vec![],
                mutable_captures: vec![],
                captures_this: false,
                captures_new_target: false,
                enclosing_class: None,
                is_arrow: false,
                is_async: false,
                is_generator: false,
                is_strict: false,
            },
        ],
    );
    let routes = Expr::Array(vec![Expr::Object(vec![
        ("name".into(), Expr::String("home".into())),
        ("body".into(), nav_button),
    ])]);
    m.init.push(app_with_body(nmc(
        "NavStack",
        vec![Expr::LocalGet(5), routes],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    // Exactly one callback registered (the Button's onClick).
    assert_eq!(r.callbacks.len(), 1);
    // The closure's body should now be a setText call (rewritten by
    // the v6 pre-walk that also runs for NavStack-nested closures).
    let captured = &r.callbacks[0];
    if let Expr::Closure { body, .. } = captured {
        let has_settext = body.iter().any(|s| {
            matches!(
                s,
                Stmt::Expr(Expr::NativeMethodCall {
                    module,
                    method,
                    ..
                }) if module == "perry/ui" && method == "setText"
            )
        });
        assert!(
            has_settext,
            "expected setText rewrite, got body: {:?}",
            body
        );
    } else {
        panic!("expected Closure callback");
    }
}

// ----- Phase 2 v6: state<T> reactive container -----

fn state_call(initial: Expr) -> Expr {
    Expr::NativeMethodCall {
        module: "perry/ui".to_string(),
        class_name: None,
        object: None,
        method: "state".to_string(),
        args: vec![initial],
    }
}

fn state_method_call(state_id: u32, method: &str, args: Vec<Expr>) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(state_id)),
            property: method.to_string(),
        }),
        args,
        type_args: vec![],
        byte_offset: 0,
    }
}

#[test]
fn state_text_emits_reactive_text_with_synth_id() {
    // const count = state(0); App({body: count.text()});
    let mut m = empty_module();
    m.init.push(Stmt::Let {
        id: 5,
        name: "count".to_string(),
        ty: perry_types::Type::Any,
        mutable: false,
        init: Some(state_call(Expr::Number(0.0))),
    });
    m.init
        .push(app_with_body(state_method_call(5, "text", vec![])));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    // Synth id is __state_0; sanitized to __state_0 (already valid).
    assert!(r.ets_source.contains("Text(this.text___state_0)"));
    // @State decl with initial value 0.
    assert!(r.ets_source.contains("@State text___state_0: string = '0'"));
}

#[test]
fn state_set_in_closure_rewrites_to_settext() {
    // const count = state(0);
    // App({body: Button("+", () => count.set(5))});
    let mut m = empty_module();
    m.init.push(Stmt::Let {
        id: 5,
        name: "count".to_string(),
        ty: perry_types::Type::Any,
        mutable: false,
        init: Some(state_call(Expr::Number(0.0))),
    });
    // Closure body: Stmt::Expr(count.set(5))
    let closure = Expr::Closure {
        func_id: 0 as perry_types::FuncId,
        params: vec![],
        return_type: perry_types::Type::Any,
        body: vec![Stmt::Expr(state_method_call(
            5,
            "set",
            vec![Expr::Number(5.0)],
        ))],
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
    m.init.push(app_with_body(nmc(
        "Button",
        vec![Expr::String("+".into()), closure],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    // The closure body should now contain a setText call. Codegen-side
    // we can't directly assert on that — but we can verify the harvest
    // captured exactly 1 callback (the rewritten closure).
    assert_eq!(r.callbacks.len(), 1);
    // And confirm the rewritten HIR has the setText shape inside.
    let captured = &r.callbacks[0];
    if let Expr::Closure { body, .. } = captured {
        let has_settext = body.iter().any(|s| {
                matches!(s, Stmt::Expr(Expr::NativeMethodCall { method, .. }) if method == "setText")
            });
        assert!(
            has_settext,
            "closure body should have been rewritten to setText"
        );
    } else {
        panic!("expected Closure in callback registry");
    }
}

#[test]
fn multiple_state_decls_get_unique_ids() {
    let mut m = empty_module();
    m.init.push(Stmt::Let {
        id: 1,
        name: "count".to_string(),
        ty: perry_types::Type::Any,
        mutable: false,
        init: Some(state_call(Expr::Number(0.0))),
    });
    m.init.push(Stmt::Let {
        id: 2,
        name: "name".to_string(),
        ty: perry_types::Type::Any,
        mutable: false,
        init: Some(state_call(Expr::String("Alice".into()))),
    });
    m.init.push(app_with_body(nmc(
        "VStack",
        vec![Expr::Array(vec![
            state_method_call(1, "text", vec![]),
            state_method_call(2, "text", vec![]),
        ])],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains("@State text___state_0: string = '0'"));
    assert!(r
        .ets_source
        .contains("@State text___state_1: string = 'Alice'"));
    assert!(r.ets_source.contains("Text(this.text___state_0)"));
    assert!(r.ets_source.contains("Text(this.text___state_1)"));
}

#[test]
fn unsupported_widget_degrades_with_comment_not_error() {
    // Use a widget that's intentionally NOT yet supported so this
    // test stays valid as the supported set grows. As of v4 we
    // still don't emit anything for `Canvas` / `Window` / `TabBar`.
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "Canvas",
        vec![Expr::Number(100.0), Expr::Number(100.0)],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r
        .ets_source
        .contains("// unsupported perry/ui widget: Canvas"));
    assert!(r.ets_source.contains("Text('[unsupported: Canvas]')"));
}

#[test]
fn image_with_src() {
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "Image",
        vec![Expr::String("logo.png".into())],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r
        .ets_source
        .contains("Image('logo.png').width('100%').height(200)"));
}

#[test]
fn imagefile_alias_emits_same_shape() {
    // ImageFile is the existing perry-ui-* TS surface name; both must
    // route through the same emitter for cross-platform parity.
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "ImageFile",
        vec![Expr::String("photo.jpg".into())],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains("Image('photo.jpg')"));
}

#[test]
fn scrollview_with_children() {
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "ScrollView",
        vec![Expr::Array(vec![
            nmc("Text", vec![Expr::String("a".into())]),
            nmc("Text", vec![Expr::String("b".into())]),
        ])],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains("Scroll() {"));
    assert!(r.ets_source.contains("Column({ space: 8 })"));
    assert!(r.ets_source.contains("Text('a').fontSize(20)"));
    assert!(r.ets_source.contains("Text('b').fontSize(20)"));
}

#[test]
fn lazyvstack_emits_column_with_deferral_comment() {
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "LazyVStack",
        vec![Expr::Array(vec![
            nmc("Text", vec![Expr::String("row 0".into())]),
            nmc("Text", vec![Expr::String("row 1".into())]),
        ])],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    // Phase 2 v10: explicit-children variant (non-ArrayMap) still
    // renders eagerly as a plain Column for backwards compat. The
    // real lazy path triggers only on `LazyVStack(items.map(...))`.
    assert!(r
        .ets_source
        .contains("LazyVStack with explicit children: rendered eagerly as Column"));
    assert!(r.ets_source.contains("Column({ space: 8 })"));
    assert!(r.ets_source.contains("Text('row 0')"));
}

// ----- Phase 2 v10: real LazyVStack with LazyForEach + IDataSource -----

#[test]
fn lazyvstack_with_array_map_emits_lazy_for_each() {
    // LazyVStack(items.map(item => Text(item)))
    let mut m = empty_module();
    let item_param = perry_hir::ir::Param {
        id: 99,
        name: "item".to_string(),
        ty: perry_types::Type::Any,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    };
    let inner_text = nmc("Text", vec![Expr::LocalGet(99)]);
    let map_expr = Expr::ArrayMap {
        array: Box::new(Expr::Array(vec![
            Expr::String("a".into()),
            Expr::String("b".into()),
        ])),
        callback: Box::new(Expr::Closure {
            func_id: 0 as perry_types::FuncId,
            params: vec![item_param],
            return_type: perry_types::Type::Any,
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
    m.init
        .push(app_with_body(nmc("LazyVStack", vec![map_expr])));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    // ArkUI shape: List() { LazyForEach(this.lazy_source_0, ...) }
    assert!(r.ets_source.contains("List() {"));
    assert!(r.ets_source.contains("LazyForEach(this.lazy_source_0"));
    assert!(r.ets_source.contains("ListItem()"));
    // Inner widget body resolves item to __item.
    assert!(r.ets_source.contains("Text(__item)"));
    // IDataSource boilerplate emitted at module top.
    assert!(r
        .ets_source
        .contains("class PerryListDataSource implements IDataSource"));
    // @State field decl on the page.
    assert!(r.ets_source.contains(
        "@State lazy_source_0: PerryListDataSource = new PerryListDataSource(['a', 'b'])"
    ));
}

#[test]
fn lazyvstack_no_array_map_skips_lazy_class_emission() {
    // Eager-mode (explicit Array) variant should NOT emit the
    // PerryListDataSource boilerplate.
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "LazyVStack",
        vec![Expr::Array(vec![nmc(
            "Text",
            vec![Expr::String("hi".into())],
        )])],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(!r.ets_source.contains("class PerryListDataSource"));
    assert!(!r.ets_source.contains("LazyForEach"));
}

#[test]
fn picker_with_options_and_closure() {
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "Picker",
        vec![
            Expr::Array(vec![
                Expr::String("Red".into()),
                Expr::String("Green".into()),
                Expr::String("Blue".into()),
            ]),
            closure_stub(),
        ],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r
        .ets_source
        .contains("TextPicker({ range: ['Red', 'Green', 'Blue'], value: 'Red' })"));
    assert!(r
        .ets_source
        .contains(".onChange((_value: string, index: number) => {"));
    assert!(r
        .ets_source
        .contains("perryEntry.invokeCallback1(0, index)"));
    assert_eq!(r.callbacks.len(), 1);
}

#[test]
fn combobox_emits_arkui_select() {
    // Issue #475 — Combobox(initial, onChange) → Select with onSelect.
    // Asserts the canonical patterns: Select( + .onSelect( + the
    // initial value used as both .value() and the only seed option.
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "Combobox",
        vec![Expr::String("Apple".into()), closure_stub()],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains("Select("));
    assert!(r.ets_source.contains(".value('Apple')"));
    assert!(r.ets_source.contains(".onSelect("));
    assert!(r
        .ets_source
        .contains("perryEntry.invokeCallback1(0, value)"));
    // Drain is wired so showToast / setText inside the closure body
    // surface after onSelect returns.
    assert!(r.ets_source.contains("perryEntry.drainToast()"));
    assert_eq!(r.callbacks.len(), 1);
}

#[test]
fn rich_text_editor_emits_arkui_richeditor() {
    // Issue #478 — RichTextEditor(width, height, onChange) emits
    // an ArkUI RichEditor with a fresh controller; width/height
    // flow through to sizing modifiers; the onChange closure is
    // captured and routed through onIMEInputComplete.
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "RichTextEditor",
        vec![Expr::Number(320.0), Expr::Number(200.0), closure_stub()],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains("RichEditor("));
    assert!(r.ets_source.contains("new RichEditorController()"));
    assert!(r.ets_source.contains(".width(320)"));
    assert!(r.ets_source.contains(".height(200)"));
    assert!(r.ets_source.contains(".onIMEInputComplete("));
    assert!(r.ets_source.contains("perryEntry.invokeCallback1(0, ''"));
    assert_eq!(r.callbacks.len(), 1);
}

#[test]
fn calendar_emits_arkui_calendar_picker() {
    // Issue #481 — Calendar(2026, 5, onChange) → CalendarPicker
    // with selected = new Date(2026, 4, 1) (month is 0-indexed in
    // JS Date) and an onChange that converts the Date payload to
    // an ISO yyyy-MM-dd string before invoking the TS callback.
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "Calendar",
        vec![Expr::Number(2026.0), Expr::Number(5.0), closure_stub()],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains("CalendarPicker("));
    // 1-based month 5 (May) → 0-based monthIndex 4
    assert!(r.ets_source.contains("new Date(2026, 4, 1)"));
    assert!(r.ets_source.contains(".onChange((value: Date) => {"));
    assert!(r.ets_source.contains("value.toISOString().split('T')[0]"));
    assert!(r
        .ets_source
        .contains("perryEntry.invokeCallback1(0, __iso)"));
    assert_eq!(r.callbacks.len(), 1);
}

#[test]
fn calendar_without_literal_args_falls_back_to_today() {
    // Calendar(yearLocal, monthLocal, _) — args don't resolve to
    // numeric literals, so the selected date defaults to `new Date()`.
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "Calendar",
        vec![
            Expr::String("not-a-number".into()),
            Expr::String("nope".into()),
            Expr::Number(0.0),
        ],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains("CalendarPicker("));
    assert!(r.ets_source.contains("selected: new Date()"));
}

#[test]
fn date_picker_emits_arkui_date_picker() {
    // Issue #4772 — DatePicker(2026, 5, onChange) → DatePicker
    // with selected = new Date(2026, 4, 1) (month is 0-indexed in
    // JS Date) and an onDateChange that converts the Date payload to
    // an ISO yyyy-MM-dd string before invoking the TS callback.
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "DatePicker",
        vec![Expr::Number(2026.0), Expr::Number(5.0), closure_stub()],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains("DatePicker("));
    // 1-based month 5 (May) → 0-based monthIndex 4
    assert!(r.ets_source.contains("new Date(2026, 4, 1)"));
    assert!(r.ets_source.contains(".onDateChange((value: Date) => {"));
    assert!(r.ets_source.contains("value.toISOString().split('T')[0]"));
    assert!(r
        .ets_source
        .contains("perryEntry.invokeCallback1(0, __iso)"));
    assert_eq!(r.callbacks.len(), 1);
}

#[test]
fn date_picker_without_literal_args_falls_back_to_today() {
    // DatePicker(yearLocal, monthLocal, _) — args don't resolve to
    // numeric literals, so the selected date defaults to `new Date()`.
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "DatePicker",
        vec![
            Expr::String("not-a-number".into()),
            Expr::String("nope".into()),
            Expr::Number(0.0),
        ],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains("DatePicker("));
    assert!(r.ets_source.contains("selected: new Date()"));
}

#[test]
fn rich_text_editor_zero_size_skips_width_height_modifiers() {
    // 0 width/height means "use intrinsic" — emitting .width(0)
    // would zero the editor. Test confirms the elision.
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "RichTextEditor",
        vec![Expr::Number(0.0), Expr::Number(0.0), Expr::Number(0.0)],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains("RichEditor("));
    assert!(!r.ets_source.contains(".width(0)"));
    assert!(!r.ets_source.contains(".height(0)"));
}

#[test]
fn progressview_with_default_value_and_total() {
    let mut m = empty_module();
    m.init.push(app_with_body(nmc("ProgressView", vec![])));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r
        .ets_source
        .contains("Progress({ value: 0, total: 100, type: ProgressType.Linear })"));
}

#[test]
fn progressview_with_explicit_value() {
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "ProgressView",
        vec![Expr::Number(42.0), Expr::Number(200.0)],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r
        .ets_source
        .contains("Progress({ value: 42, total: 200, type: ProgressType.Linear })"));
}

#[test]
fn section_with_title_and_children() {
    let mut m = empty_module();
    m.init.push(app_with_body(nmc(
        "Section",
        vec![
            Expr::String("Personal Info".into()),
            Expr::Array(vec![
                nmc("Text", vec![Expr::String("name".into())]),
                nmc("Text", vec![Expr::String("email".into())]),
            ]),
        ],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains("Column({ space: 4 })"));
    assert!(r
        .ets_source
        .contains("Text('Personal Info').fontSize(14).fontColor('#888888')"));
    assert!(r.ets_source.contains("Text('name').fontSize(20)"));
    assert!(r.ets_source.contains("Text('email').fontSize(20)"));
}

#[test]
fn string_literal_escaping() {
    assert_eq!(arkts_string_lit("hi"), "'hi'");
    assert_eq!(arkts_string_lit("he's there"), "'he\\'s there'");
    assert_eq!(arkts_string_lit("a\\b"), "'a\\\\b'");
    assert_eq!(arkts_string_lit("line1\nline2"), "'line1\\nline2'");
}

#[test]
fn fmt_num_drops_decimal_for_whole_numbers() {
    assert_eq!(fmt_num(8.0), "8");
    assert_eq!(fmt_num(16.0), "16");
    assert_eq!(fmt_num(1.5), "1.5");
    assert_eq!(fmt_num(-3.0), "-3");
}

// ─── #369 perry/media drain glue ────────────────────────────────

fn media_call(method: &str, args: Vec<Expr>) -> Expr {
    Expr::NativeMethodCall {
        module: "perry/media".to_string(),
        class_name: None,
        object: None,
        method: method.to_string(),
        args,
    }
}

#[test]
fn no_media_use_omits_media_glue() {
    let mut m = empty_module();
    m.init
        .push(app_with_body(nmc("Text", vec![Expr::String("hi".into())])));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(!r.ets_source.contains("@ohos.multimedia.media"));
    assert!(!r.ets_source.contains("mediaPlayers"));
    assert!(!r.ets_source.contains("runMediaPump"));
}

#[test]
fn createplayer_in_init_emits_media_glue() {
    // `createPlayer(url)` is a top-level call (not inside App body),
    // typical media-app shape: `const p = createPlayer(url); App({body: ...})`.
    let mut m = empty_module();
    m.init.push(Stmt::Expr(media_call(
        "createPlayer",
        vec![Expr::String("https://e.x/a.mp3".into())],
    )));
    m.init
        .push(app_with_body(nmc("Text", vec![Expr::String("hi".into())])));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    // Imports.
    assert!(r
        .ets_source
        .contains("import media from '@ohos.multimedia.media'"));
    // Per-instance state.
    assert!(r
        .ets_source
        .contains("private mediaPlayers: Map<number, media.AVPlayer>"));
    // Lifecycle pump.
    assert!(r.ets_source.contains("aboutToAppear()"));
    assert!(r
        .ets_source
        .contains("setInterval(() => { this.runMediaPump(); }, 100)"));
    // Three drain loops.
    assert!(r.ets_source.contains("perryEntry.drainMediaCreate()"));
    assert!(r.ets_source.contains("perryEntry.drainMediaControl()"));
    assert!(r.ets_source.contains("perryEntry.drainNowPlaying()"));
    // State pushback.
    assert!(r.ets_source.contains("perryEntry.pushMediaState"));
    // AVPlayer dispatch.
    assert!(r.ets_source.contains("media.createAVPlayer()"));
    assert!(r.ets_source.contains("player.play()"));
    assert!(r.ets_source.contains("player.pause()"));
    assert!(r.ets_source.contains("player.seek("));
    assert!(r.ets_source.contains("player.setVolume("));
    assert!(r.ets_source.contains("player.release()"));
}

#[test]
fn media_call_inside_button_closure_also_triggers_glue() {
    // Critical for play/pause buttons: the perry/media calls live
    // inside Button's onClick closure, not in module.init. The
    // walker must descend into Closure bodies via stmt_uses → Closure.
    let mut m = empty_module();
    let play_closure = Expr::Closure {
        func_id: 0 as perry_types::FuncId,
        params: vec![],
        return_type: perry_types::Type::Any,
        body: vec![Stmt::Expr(media_call("play", vec![Expr::Number(1.0)]))],
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
    m.init.push(app_with_body(nmc(
        "Button",
        vec![Expr::String("Play".into()), play_closure],
    )));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r
        .ets_source
        .contains("import media from '@ohos.multimedia.media'"));
    assert!(r.ets_source.contains("runMediaPump"));
}

// ─── #408 procedural mutation tracking ─────────────────────────────

/// Helper: Let-bind a widget to a LocalId so mutator calls can target it.
fn let_widget(id: LocalId, name: &str, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: perry_types::Type::Any,
        mutable: false,
        init: Some(init),
    }
}

/// Helper: a perry/ui mutator call expression, e.g. widgetAddChild(parent, child).
fn mutator_stmt(method: &str, args: Vec<Expr>) -> Stmt {
    Stmt::Expr(Expr::NativeMethodCall {
        module: "perry/ui".to_string(),
        class_name: None,
        object: None,
        method: method.to_string(),
        args,
    })
}

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

// ----------------------------------------------------------------
// Issue #410 — emitted ArkUI must compile cleanly through ArkTS.
//
// The three bugs documented in the issue:
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
// These regression tests pin the emitted-source invariants:
//   - never the substring `__local_`
//   - never a `*/` inside a `/* if ((...)) */` marker
//   - `__platform__` comparisons inline as numeric literals (9 for
//     harmonyos, the only target this codegen serves).
// ----------------------------------------------------------------

/// Helper: declare-const stmt for `__platform__` (the canonical HIR
/// shape `Stmt::Let { name, init: None }` — the same shape
/// `crates/perry-codegen/src/codegen.rs::compile_time_constants`
/// recognizes).
fn declare_const(id: LocalId, name: &str) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: perry_types::Type::Any,
        mutable: false,
        init: None,
    }
}

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

/// Walk the source line-by-line and assert no line opens a `/*` that
/// contains a second `*/` after the first one (which would break
/// parsing). This is a tighter form of "no `*/` inside `/* ... */`":
/// for every block-comment marker, count the number of `*/` between
/// `/*` and the next `*/` — must be exactly one.
fn assert_no_nested_block_comments(src: &str) {
    let mut i = 0;
    let bytes = src.as_bytes();
    while i + 1 < bytes.len() {
        if bytes[i] == b'/' && bytes[i + 1] == b'*' {
            // Found an opening `/*`. Find the matching close.
            let start = i;
            i += 2;
            let mut close = None;
            while i + 1 < bytes.len() {
                if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    close = Some(i);
                    break;
                }
                i += 1;
            }
            let Some(close) = close else { return };
            // The comment body is bytes[start+2..close]. It must NOT
            // itself contain a `*/` (which would mean the original
            // close was actually the *second* close — impossible per
            // the inner-loop logic above, but the symmetric check
            // catches the other failure mode where serialize_condition
            // smuggled in a `*/` that was treated as the close.
            let body = &src[start + 2..close];
            assert!(
                !body.contains("*/"),
                "nested block comment found at {}: body={:?}\nfull source:\n{}",
                start,
                body,
                src
            );
            i = close + 2;
        } else {
            i += 1;
        }
    }
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

// ------------------------------------------------------------------
// Issue #669 — Chart on HarmonyOS (ArkUI Canvas backend).
// ------------------------------------------------------------------

#[test]
fn chart_bar_with_data_points_emits_canvas_and_draw_calls() {
    // const c = Chart(1, 200, 150);
    // chartAddDataPoint(c, 'Q1', 10);
    // chartAddDataPoint(c, 'Q2', 20);
    // chartSetTitle(c, 'Sales');
    // App({ body: c });
    let mut m = empty_module();
    m.init.push(let_widget(
        42,
        "c",
        nmc(
            "Chart",
            vec![Expr::Integer(1), Expr::Number(200.0), Expr::Number(150.0)],
        ),
    ));
    m.init.push(mutator_stmt(
        "chartAddDataPoint",
        vec![
            Expr::LocalGet(42),
            Expr::String("Q1".into()),
            Expr::Number(10.0),
        ],
    ));
    m.init.push(mutator_stmt(
        "chartAddDataPoint",
        vec![
            Expr::LocalGet(42),
            Expr::String("Q2".into()),
            Expr::Number(20.0),
        ],
    ));
    m.init.push(mutator_stmt(
        "chartSetTitle",
        vec![Expr::LocalGet(42), Expr::String("Sales".into())],
    ));
    m.init.push(app_with_body(Expr::LocalGet(42)));

    let r = emit_index_ets(&mut m).unwrap().unwrap();
    let src = r.ets_source;

    // Canvas widget + per-instance ctx field.
    assert!(
        src.contains("Canvas(this.__chart_0_ctx)"),
        "Canvas with per-instance ctx must render:\n{}",
        src,
    );
    assert!(
        src.contains(
            "private __chart_0_settings: RenderingContextSettings = \
                 new RenderingContextSettings(true)"
        ),
        "RenderingContextSettings field missing:\n{}",
        src,
    );
    assert!(
        src.contains(
            "private __chart_0_ctx: CanvasRenderingContext2D = \
                 new CanvasRenderingContext2D(this.__chart_0_settings)"
        ),
        "CanvasRenderingContext2D field missing:\n{}",
        src,
    );
    // Size flowed through.
    assert!(src.contains(".width(200)"), "width missing:\n{}", src);
    assert!(src.contains(".height(150)"), "height missing:\n{}", src);
    // Data points folded.
    assert!(
        src.contains("{ label: 'Q1', value: 10 }"),
        "Q1 point missing:\n{}",
        src
    );
    assert!(
        src.contains("{ label: 'Q2', value: 20 }"),
        "Q2 point missing:\n{}",
        src
    );
    // Title folded.
    assert!(
        src.contains("const title: string = 'Sales'"),
        "title missing:\n{}",
        src
    );
    // 2D context draw calls present (bar branch uses fillRect for bars).
    assert!(
        src.contains("ctx.clearRect(0, 0, cw, ch)"),
        "clearRect missing:\n{}",
        src
    );
    assert!(src.contains("ctx.fillRect("), "fillRect missing:\n{}", src);
    assert!(
        src.contains("ctx.fillText(title, cw / 2, 22)"),
        "title fillText missing:\n{}",
        src
    );
}

#[test]
fn chart_line_kind_emits_stroke_path() {
    let mut m = empty_module();
    m.init.push(let_widget(
        7,
        "c",
        nmc(
            "Chart",
            vec![Expr::Integer(0), Expr::Number(100.0), Expr::Number(100.0)],
        ),
    ));
    m.init.push(mutator_stmt(
        "chartAddDataPoint",
        vec![
            Expr::LocalGet(7),
            Expr::String("a".into()),
            Expr::Number(5.0),
        ],
    ));
    m.init.push(app_with_body(Expr::LocalGet(7)));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    let src = r.ets_source;
    // Line kind: lineTo + stroke + arc-dots.
    assert!(src.contains("ctx.lineTo("), "lineTo missing:\n{}", src);
    assert!(src.contains("ctx.stroke()"), "stroke() missing:\n{}", src);
    assert!(src.contains("ctx.arc("), "arc dot missing:\n{}", src);
}

#[test]
fn chart_pie_kind_emits_arc_fill_and_legend() {
    let mut m = empty_module();
    m.init.push(let_widget(
        9,
        "c",
        nmc(
            "Chart",
            vec![Expr::Integer(2), Expr::Number(120.0), Expr::Number(120.0)],
        ),
    ));
    m.init.push(mutator_stmt(
        "chartAddDataPoint",
        vec![
            Expr::LocalGet(9),
            Expr::String("x".into()),
            Expr::Number(1.0),
        ],
    ));
    m.init.push(app_with_body(Expr::LocalGet(9)));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    let src = r.ets_source;
    assert!(
        src.contains("ctx.arc(cx, cy, radius"),
        "pie arc missing:\n{}",
        src
    );
    assert!(
        src.contains("ctx.closePath()"),
        "pie closePath missing:\n{}",
        src
    );
    assert!(src.contains("ctx.fill()"), "pie fill missing:\n{}", src);
}

#[test]
fn chart_clear_data_resets_points() {
    // chartAddDataPoint then chartClearData then chartAddDataPoint —
    // only the last point should survive in the static fold.
    let mut m = empty_module();
    m.init.push(let_widget(
        5,
        "c",
        nmc(
            "Chart",
            vec![Expr::Integer(1), Expr::Number(100.0), Expr::Number(100.0)],
        ),
    ));
    m.init.push(mutator_stmt(
        "chartAddDataPoint",
        vec![
            Expr::LocalGet(5),
            Expr::String("dropped".into()),
            Expr::Number(99.0),
        ],
    ));
    m.init
        .push(mutator_stmt("chartClearData", vec![Expr::LocalGet(5)]));
    m.init.push(mutator_stmt(
        "chartAddDataPoint",
        vec![
            Expr::LocalGet(5),
            Expr::String("kept".into()),
            Expr::Number(7.0),
        ],
    ));
    m.init.push(app_with_body(Expr::LocalGet(5)));

    let r = emit_index_ets(&mut m).unwrap().unwrap();
    let src = r.ets_source;
    assert!(
        !src.contains("'dropped'"),
        "cleared point must not render:\n{}",
        src
    );
    assert!(
        src.contains("{ label: 'kept', value: 7 }"),
        "surviving point must render:\n{}",
        src
    );
}

// ------------------------------------------------------------------
// Issue #670 — TreeView on HarmonyOS (ArkUI List backend).
// ------------------------------------------------------------------

#[test]
fn treeview_static_graph_emits_list_foreach_and_state() {
    // const root  = TreeNode('root', 'Root');
    // const child = TreeNode('c1',   'Child 1');
    // treeNodeAddChild(root, child);
    // const tv = TreeView(root, () => {});
    // App({ body: tv });
    let mut m = empty_module();
    m.init.push(let_widget(
        10,
        "root",
        nmc(
            "TreeNode",
            vec![Expr::String("root".into()), Expr::String("Root".into())],
        ),
    ));
    m.init.push(let_widget(
        11,
        "child",
        nmc(
            "TreeNode",
            vec![Expr::String("c1".into()), Expr::String("Child 1".into())],
        ),
    ));
    m.init.push(mutator_stmt(
        "treeNodeAddChild",
        vec![Expr::LocalGet(10), Expr::LocalGet(11)],
    ));
    m.init.push(let_widget(
        12,
        "tv",
        nmc("TreeView", vec![Expr::LocalGet(10), closure_stub()]),
    ));
    m.init.push(app_with_body(Expr::LocalGet(12)));

    let r = emit_index_ets(&mut m).unwrap().unwrap();
    let src = r.ets_source;

    // List + ForEach with the flatten helper as its source.
    assert!(
        src.contains("List({ space: 0 })"),
        "List container missing:\n{}",
        src,
    );
    assert!(
        src.contains("ForEach(this.__tree_0_flatten(),"),
        "ForEach over flatten missing:\n{}",
        src,
    );
    // Static node data baked recursively (root holds child).
    assert!(
        src.contains(
            "{ id: 'root', label: 'Root', \
                 children: [{ id: 'c1', label: 'Child 1', children: [] }] }"
        ),
        "recursive node literal missing:\n{}",
        src,
    );
    // @State fields for expanded set + selected id.
    assert!(
        src.contains("@State __tree_0_expanded: Set<string> = new Set<string>()"),
        "expanded @State missing:\n{}",
        src,
    );
    assert!(
        src.contains("@State __tree_0_selectedId: string = ''"),
        "selectedId @State missing:\n{}",
        src,
    );
    // Flatten method emitted on the @Component.
    assert!(
        src.contains("__tree_0_flatten():"),
        "flatten helper missing:\n{}",
        src,
    );
    // Tap-handler wires invokeCallback1 with row.id.
    assert!(
        src.contains("perryEntry.invokeCallback1(0, row.id)"),
        "onSelect dispatch missing:\n{}",
        src,
    );
    assert_eq!(r.callbacks.len(), 1);
}

#[test]
fn treeview_depth_padding_uses_row_depth_field() {
    // Verifies the ArkUI .padding({ left: row.depth * 16 }) shape so
    // children render with their indent. The actual numbers (16 px)
    // are a v1 layout choice — change requires test + code together.
    let mut m = empty_module();
    m.init.push(let_widget(
        20,
        "root",
        nmc(
            "TreeNode",
            vec![Expr::String("r".into()), Expr::String("R".into())],
        ),
    ));
    m.init.push(let_widget(
        21,
        "tv",
        nmc("TreeView", vec![Expr::LocalGet(20), closure_stub()]),
    ));
    m.init.push(app_with_body(Expr::LocalGet(21)));
    let r = emit_index_ets(&mut m).unwrap().unwrap();
    let src = r.ets_source;
    assert!(
        src.contains(".padding({ left: row.depth * 16,"),
        "depth-based padding missing:\n{}",
        src,
    );
}
