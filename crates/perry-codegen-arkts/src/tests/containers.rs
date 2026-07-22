// Container / composite widget tests: Tabs/Menu/Grid/Modal, NavStack
// navigation, state<T> reactivity, ScrollView/LazyVStack, pickers and
// editors (Picker/Combobox/RichTextEditor/Calendar/DatePicker), Progress/
// Section, string + number formatting, and the perry/media drain glue.
use super::*;

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
        ty: perry_hir::types::Type::Any,
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
        ty: perry_hir::types::Type::Any,
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
        ty: perry_hir::types::Type::Any,
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
        ty: perry_hir::types::Type::Any,
        mutable: false,
        init: Some(state_call(Expr::String("home".into()))),
    });
    let nav_button = nmc(
        "Button",
        vec![
            Expr::String("Go".into()),
            Expr::Closure {
                func_id: 0 as perry_hir::types::FuncId,
                params: vec![],
                return_type: perry_hir::types::Type::Any,
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

#[test]
fn state_text_emits_reactive_text_with_synth_id() {
    // const count = state(0); App({body: count.text()});
    let mut m = empty_module();
    m.init.push(Stmt::Let {
        id: 5,
        name: "count".to_string(),
        ty: perry_hir::types::Type::Any,
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
        ty: perry_hir::types::Type::Any,
        mutable: false,
        init: Some(state_call(Expr::Number(0.0))),
    });
    // Closure body: Stmt::Expr(count.set(5))
    let closure = Expr::Closure {
        func_id: 0 as perry_hir::types::FuncId,
        params: vec![],
        return_type: perry_hir::types::Type::Any,
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
        ty: perry_hir::types::Type::Any,
        mutable: false,
        init: Some(state_call(Expr::Number(0.0))),
    });
    m.init.push(Stmt::Let {
        id: 2,
        name: "name".to_string(),
        ty: perry_hir::types::Type::Any,
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
        ty: perry_hir::types::Type::Any,
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
        func_id: 0 as perry_hir::types::FuncId,
        params: vec![],
        return_type: perry_hir::types::Type::Any,
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
