//! Phase 2 v9 — comprehensive ArkUI emission smoke test.
//!
//! Constructs a single `Module` that exercises every Phase 2 widget shape
//! (counter app with `state<T>`, Tabs, Menu, Grid, LazyVStack with `.map`,
//! ForEach via array.map, inline `style: { ... }` with animation/shadow/
//! textDecoration, `@app.media/X` image, Toggle/TextField/Slider with
//! closures producing multi-arg invokeCallback1) and asserts the emitted
//! `Index.ets` contains every canonical pattern Phase 2 v2-v13 added.
//!
//! Acts as the integration-level "the whole stack still works together"
//! guard. Each widget has its own unit test in `lib.rs::tests`; this file
//! is the cross-cutting check that every widget composes correctly in one
//! page.
//!
//! Wired into CI as a discrete `harmonyos-smoke` job in `.github/workflows/
//! test.yml` so a regression in any one widget surfaces as a single red
//! cell distinct from the broader cargo-test job.
//!
//! End-to-end `perry compile --target harmonyos` is NOT part of this test
//! — it requires the OpenHarmony SDK which isn't present on ubuntu-latest
//! runners (downloading the ~600 MB SDK every CI run isn't worth the
//! signal). The codegen path that produces `Index.ets` runs entirely
//! through `perry_codegen_arkts::emit_index_ets`, which is what this test
//! drives. Linker-side validation is covered by manual on-device runs in
//! the v0.5.399+ harmonyos test suite.

use perry_codegen_arkts::emit_index_ets;
use perry_hir::ir::{Expr, Module, Param, Stmt};
use perry_hir::types::{FuncId, LocalId, Type};

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
        annexb_global_undefined_names: Vec::new(),
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
        class_display_names: std::collections::HashMap::new(),
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

fn closure(params: Vec<Param>, body: Vec<Stmt>) -> Expr {
    Expr::Closure {
        func_id: 0 as FuncId,
        params,
        return_type: Type::Any,
        body,
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

fn param(id: LocalId, name: &str) -> Param {
    Param {
        id,
        name: name.to_string(),
        ty: Type::Any,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
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

/// Comprehensive Phase 2 app: every widget shape from v2 through v13
/// composed into a single page.
#[test]
fn full_phase2_app_emits_canonical_arkui() {
    let mut m = empty_module();

    // --- Phase 2 v6: state<T>(0) for reactive counter ---
    // let counter = state(0);
    let counter_id: LocalId = 100;
    m.init.push(Stmt::Let {
        id: counter_id,
        name: "counter".to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(Expr::NativeMethodCall {
            module: "perry/ui".to_string(),
            class_name: None,
            object: None,
            method: "state".to_string(),
            args: vec![Expr::Number(0.0)],
        }),
    });

    // --- Phase 2 v3.2: reactive Text via Text("Count: 0", "label") ---
    let counter_label = nmc(
        "Text",
        vec![
            Expr::String("Count: 0".into()),
            Expr::String("label".into()),
        ],
    );

    // --- Phase 2 v6: state<T>.text() — reactive Text bound to synth id ---
    let counter_state_text = Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            byte_offset: 0,
            object: Box::new(Expr::LocalGet(counter_id)),
            property: "text".to_string(),
        }),
        args: vec![],
        type_args: vec![],
        byte_offset: 0,
    };

    // --- Phase 2 v6 + v5: Button with state.set + inline style ---
    // Button("Inc", () => { counter.set(counter.value + 1) }, { backgroundColor: "blue", borderRadius: 8 })
    let inc_body = vec![Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            byte_offset: 0,
            object: Box::new(Expr::LocalGet(counter_id)),
            property: "set".to_string(),
        }),
        args: vec![Expr::Number(1.0)],
        type_args: vec![],
        byte_offset: 0,
    })];
    let inc_button = nmc(
        "Button",
        vec![
            Expr::String("Inc".into()),
            closure(vec![], inc_body),
            Expr::Object(vec![
                ("backgroundColor".into(), Expr::String("blue".into())),
                ("borderRadius".into(), Expr::Number(8.0)),
            ]),
        ],
    );

    // --- Phase 2 v13: animation modifier on a Text ---
    let animated_text = nmc(
        "Text",
        vec![
            Expr::String("Animated".into()),
            Expr::Object(vec![(
                "animation".into(),
                Expr::Object(vec![
                    ("duration".into(), Expr::Number(300.0)),
                    ("curve".into(), Expr::String("easeOut".into())),
                ]),
            )]),
        ],
    );

    // --- Phase 2 v13: shadow modifier ---
    let shadowed_button = nmc(
        "Button",
        vec![
            Expr::String("Shadowed".into()),
            closure(vec![], vec![]),
            Expr::Object(vec![(
                "shadow".into(),
                Expr::Object(vec![
                    ("blur".into(), Expr::Number(12.0)),
                    ("offsetX".into(), Expr::Number(0.0)),
                    ("offsetY".into(), Expr::Number(4.0)),
                ]),
            )]),
        ],
    );

    // --- Phase 2 v13: image with @app.media path ---
    let icon = nmc("Image", vec![Expr::String("@app.media/icon".into())]);

    // --- Phase 2 v13: textDecoration ---
    let underlined_text = nmc(
        "Text",
        vec![
            Expr::String("Underlined".into()),
            Expr::Object(vec![(
                "textDecoration".into(),
                Expr::String("underline".into()),
            )]),
        ],
    );

    // --- Phase 2 v2.5: Toggle with closure (multi-arg invokeCallback1) ---
    let toggle = nmc(
        "Toggle",
        vec![
            Expr::String("Notifications".into()),
            Expr::Bool(false),
            closure(vec![param(200, "v")], vec![]),
        ],
    );

    // --- Phase 2 v2.5: TextField with closure ---
    let textfield = nmc(
        "TextField",
        vec![
            Expr::String("Name".into()),
            closure(vec![param(201, "txt")], vec![]),
        ],
    );

    // --- Phase 2 v2.5: Slider with closure ---
    let slider = nmc(
        "Slider",
        vec![
            Expr::Number(0.0),
            Expr::Number(100.0),
            closure(vec![param(202, "n")], vec![]),
        ],
    );

    // --- Phase 2 v5: ForEach via array.map(item => Text(item)) ---
    let item_id: LocalId = 300;
    let foreach_block = Expr::ArrayMap {
        array: Box::new(Expr::Array(vec![
            Expr::String("alpha".into()),
            Expr::String("beta".into()),
            Expr::String("gamma".into()),
        ])),
        callback: Box::new(closure(
            vec![param(item_id, "item")],
            vec![Stmt::Return(Some(nmc(
                "Text",
                vec![Expr::LocalGet(item_id)],
            )))],
        )),
    };

    // --- Phase 2 v10: real LazyVStack via items.map ---
    let lazy_item_id: LocalId = 400;
    let lazy_block = nmc(
        "LazyVStack",
        vec![Expr::ArrayMap {
            array: Box::new(Expr::Array(vec![
                Expr::String("row1".into()),
                Expr::String("row2".into()),
                Expr::String("row3".into()),
            ])),
            callback: Box::new(closure(
                vec![param(lazy_item_id, "row")],
                vec![Stmt::Return(Some(nmc(
                    "Text",
                    vec![Expr::LocalGet(lazy_item_id)],
                )))],
            )),
        }],
    );

    // --- Phase 2 v12: Tabs ---
    let tabs = nmc(
        "Tabs",
        vec![Expr::Array(vec![
            Expr::Object(vec![
                ("label".into(), Expr::String("Home".into())),
                (
                    "body".into(),
                    nmc("Text", vec![Expr::String("home content".into())]),
                ),
            ]),
            Expr::Object(vec![
                ("label".into(), Expr::String("About".into())),
                (
                    "body".into(),
                    nmc("Text", vec![Expr::String("about content".into())]),
                ),
            ]),
        ])],
    );

    // --- Phase 2 v12: Menu (Column of Buttons) ---
    let menu = nmc(
        "Menu",
        vec![Expr::Array(vec![
            nmc(
                "Button",
                vec![Expr::String("Save".into()), closure(vec![], vec![])],
            ),
            nmc(
                "Button",
                vec![Expr::String("Cancel".into()), closure(vec![], vec![])],
            ),
        ])],
    );

    // --- Phase 2 v12: Grid ---
    let grid = nmc(
        "Grid",
        vec![
            Expr::Number(2.0),
            Expr::Array(vec![
                nmc("Text", vec![Expr::String("cell1".into())]),
                nmc("Text", vec![Expr::String("cell2".into())]),
                nmc("Text", vec![Expr::String("cell3".into())]),
                nmc("Text", vec![Expr::String("cell4".into())]),
            ]),
        ],
    );

    // Compose into a top-level VStack.
    let body = nmc(
        "VStack",
        vec![Expr::Array(vec![
            counter_label,
            counter_state_text,
            inc_button,
            animated_text,
            shadowed_button,
            icon,
            underlined_text,
            toggle,
            textfield,
            slider,
            foreach_block,
            lazy_block,
            tabs,
            menu,
            grid,
        ])],
    );

    m.init.push(app_with_body(body));

    let result = emit_index_ets(&mut m).expect("harvest must succeed");
    let harvest = result.expect("App({...}) must produce a HarvestResult");
    let ets = &harvest.ets_source;

    // ---- Canonical structural patterns ----
    // Page wrapper.
    assert!(
        ets.contains("@Entry") && ets.contains("@Component") && ets.contains("struct Index"),
        "missing @Entry @Component struct Index page wrapper:\n{}",
        ets
    );

    // Phase 2 v6: state<T> field declarations.
    assert!(
        ets.contains("@State text___state_0"),
        "missing v6 state<T> @State field:\n{}",
        ets
    );

    // Phase 2 v3.2: explicit-id reactive Text field.
    assert!(
        ets.contains("@State text_label"),
        "missing v3.2 reactive-Text @State field:\n{}",
        ets
    );
    assert!(
        ets.contains("Text(this.text_label)"),
        "missing v3.2 reactive-Text widget reference:\n{}",
        ets
    );

    // Phase 2 v5: inline style modifier chain on Button.
    assert!(
        ets.contains(".backgroundColor('blue')"),
        "missing v5 inline-style backgroundColor:\n{}",
        ets
    );
    assert!(
        ets.contains(".borderRadius(8)"),
        "missing v5 inline-style borderRadius:\n{}",
        ets
    );

    // Phase 2 v13: animation modifier with curve enum.
    assert!(
        ets.contains(".animation({ duration: 300, curve: Curve.EaseOut })"),
        "missing v13 animation modifier:\n{}",
        ets
    );

    // Phase 2 v13: shadow modifier (blur → radius, offsets).
    assert!(
        ets.contains(".shadow("),
        "missing v13 shadow modifier:\n{}",
        ets
    );

    // Phase 2 v13: image asset resource accessor.
    assert!(
        ets.contains("Image($r('app.media.icon'))"),
        "missing v13 @app.media → $r() accessor:\n{}",
        ets
    );

    // Phase 2 v13: textDecoration → ArkUI decoration modifier.
    assert!(
        ets.contains(".decoration("),
        "missing v13 textDecoration → decoration modifier:\n{}",
        ets
    );

    // Phase 2 v2.5: multi-arg callbacks via invokeCallback1.
    assert!(
        ets.contains("invokeCallback1"),
        "missing v2.5 invokeCallback1 NAPI bridge call:\n{}",
        ets
    );

    // Phase 2 v5: ForEach via array.map.
    assert!(
        ets.contains("ForEach(['alpha', 'beta', 'gamma']"),
        "missing v5 ForEach with array literal:\n{}",
        ets
    );

    // Phase 2 v10: real LazyVStack (List + LazyForEach + IDataSource).
    assert!(
        ets.contains("List() {") && ets.contains("LazyForEach"),
        "missing v10 LazyVStack → List + LazyForEach:\n{}",
        ets
    );
    assert!(
        ets.contains("class PerryListDataSource"),
        "missing v10 PerryListDataSource boilerplate class:\n{}",
        ets
    );
    assert!(
        ets.contains("@State lazy_source_"),
        "missing v10 @State lazy_source_<N> field decl:\n{}",
        ets
    );

    // Phase 2 v12: Tabs / Menu / Grid.
    assert!(
        ets.contains("Tabs() {"),
        "missing v12 Tabs container:\n{}",
        ets
    );
    assert!(
        ets.contains(".tabBar('Home')") && ets.contains(".tabBar('About')"),
        "missing v12 Tabs .tabBar labels:\n{}",
        ets
    );
    assert!(
        ets.contains("Grid() {"),
        "missing v12 Grid container:\n{}",
        ets
    );
    assert!(
        ets.contains(".columnsTemplate("),
        "missing v12 Grid columnsTemplate:\n{}",
        ets
    );

    // Phase 2 v2: callback registrations harvested for runtime bridge.
    assert!(
        !harvest.callbacks.is_empty(),
        "expected at least one harvested callback (Inc button + Toggle/TextField/Slider/Menu)"
    );
}

/// Smaller smoke that asserts the emit pipeline exits cleanly on a
/// minimal-but-non-trivial app (counter only). Catches regressions where
/// the state<T> rewrite or the reactive-Text harvest break the fast path.
#[test]
fn minimal_counter_app_emits_clean_page() {
    let mut m = empty_module();
    let counter_id: LocalId = 100;
    m.init.push(Stmt::Let {
        id: counter_id,
        name: "c".to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(Expr::NativeMethodCall {
            module: "perry/ui".to_string(),
            class_name: None,
            object: None,
            method: "state".to_string(),
            args: vec![Expr::Number(0.0)],
        }),
    });

    let body = nmc(
        "VStack",
        vec![Expr::Array(vec![
            // c.text() → reactive Text bound to the synth id.
            Expr::Call {
                callee: Box::new(Expr::PropertyGet {
                    byte_offset: 0,
                    object: Box::new(Expr::LocalGet(counter_id)),
                    property: "text".to_string(),
                }),
                args: vec![],
                type_args: vec![],
                byte_offset: 0,
            },
            nmc(
                "Button",
                vec![Expr::String("+".into()), closure(vec![], vec![])],
            ),
        ])],
    );
    m.init.push(app_with_body(body));

    let r = emit_index_ets(&mut m).unwrap().unwrap();
    assert!(r.ets_source.contains("@Entry"));
    assert!(r.ets_source.contains("Text(this.text___state_0)"));
    assert!(r.ets_source.contains("Button('+')"));
}
