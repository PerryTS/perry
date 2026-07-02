// Issue #669 — Chart on HarmonyOS (ArkUI Canvas backend).
// Issue #670 — TreeView on HarmonyOS (ArkUI List backend).
use super::*;

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
