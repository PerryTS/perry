// This module is part of the perry-codegen-arkts crate. It was
// mechanically split out of the former monolithic lib.rs (issue
// #1100). Pure code move — no logic changes.
#![allow(clippy::too_many_arguments)]
use crate::*;

/// Issue #670 — `TreeView(rootNode, onSelect)` → ArkUI `List` with
/// a recursive flatten. The node graph is rebuilt at codegen time by
/// chasing `rootNode` back through `bindings` to its `TreeNode(id,
/// label)` constructor + accumulated `treeNodeAddChild` mutations.
/// The result lives as a `private __tree_<n>_nodes: TreeNodeData[]`
/// constant on the @Component; expand/collapse state is
/// `@State __tree_<n>_expanded: Set<string>`; the currently selected
/// id is `@State __tree_<n>_selectedId: string`. A helper method
/// `__tree_<n>_flatten()` walks the graph using the expanded set and
/// returns the visible rows ArkUI's `ForEach` iterates over.
///
/// Tapping a row that has children toggles the chevron and re-renders
/// (ArkUI reacts to the Set mutation via the `=` assignment helper).
/// Tapping any row fires the user `onSelect` closure with the row's id.
/// `treeViewExpandAll` / `treeViewCollapseAll` / `treeViewGetSelectedId`
/// route through the existing perry_arkts drain queue at runtime — out
/// of scope for v1; the @State surface is in place so a v1.1 NAPI
/// drain bridge can flip the set without touching codegen.
pub(crate) fn emit_treeview(
    args: &[Expr],
    bindings: &HashMap<LocalId, Expr>,
    callbacks: &mut Vec<Expr>,
    mutations: &HashMap<LocalId, Vec<MutationEntry>>,
    extras: &mut HarvestExtras,
) -> String {
    let root_arg = args.first();

    let on_select_idx = match args.get(1) {
        Some(closure @ Expr::Closure { .. }) => {
            let idx = callbacks.len();
            callbacks.push(closure.clone());
            Some(idx)
        }
        _ => None,
    };

    // Build the static tree by walking from root_arg through bindings.
    let root = root_arg.and_then(|e| build_tree_node(e, bindings, mutations, 0));
    let Some(root_node) = root else {
        // Couldn't resolve the root TreeNode call — degrade to a comment
        // + placeholder so the user can see the gap.
        return format!(
            "// TreeView: couldn't resolve root TreeNode call (non-literal binding)\n\
             Text('[TreeView: unresolved root]').fontSize(14).fontColor('#888888')"
        );
    };

    let field_id = format!("{}", extras.tree_view_instances.len());
    extras.tree_view_instances.push(TreeViewInstance {
        field_id: field_id.clone(),
        root: root_node,
        on_select_idx,
    });

    let select_call = match on_select_idx {
        Some(idx) => format!(
            "perryEntry.invokeCallback1({}, row.id); {drain}",
            idx,
            drain = drain_loop_body()
        ),
        None => String::new(),
    };

    // ForEach key extractor uses row.id so ArkUI can diff between
    // pre/post-expand flattenings. The chevron leaf-vs-branch logic
    // mirrors the Android indentation glyphs ("▾"/"▸"/blank).
    format!(
        "List({{ space: 0 }}) {{\n\
         \x20\x20\x20\x20ForEach(this.__tree_{id}_flatten(), (row: {{ id: string, label: string, depth: number, hasChildren: boolean, expanded: boolean }}) => {{\n\
         \x20\x20\x20\x20\x20\x20\x20\x20ListItem() {{\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20Row({{ space: 4 }}) {{\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20Text(row.hasChildren ? (row.expanded ? '\u{25be}' : '\u{25b8}') : ' ').fontSize(14).width(16)\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20Text(row.label).fontSize(14)\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20}}\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20.padding({{ left: row.depth * 16, top: 4, bottom: 4 }})\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20.onClick(() => {{\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20this.__tree_{id}_selectedId = row.id;\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20if (row.hasChildren) {{\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20const next = new Set<string>(this.__tree_{id}_expanded);\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20if (next.has(row.id)) {{ next.delete(row.id); }} else {{ next.add(row.id); }}\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20this.__tree_{id}_expanded = next;\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20}}\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20{select}\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20}})\n\
         \x20\x20\x20\x20\x20\x20\x20\x20}}\n\
         \x20\x20\x20\x20}}, (row: {{ id: string }}) => row.id)\n\
         }}",
        id = field_id,
        select = select_call,
    )
}

/// Walk an `Expr` (presumably a `LocalGet` referring to a `TreeNode(id,
/// label)` constructor) through bindings + recorded `treeNodeAddChild`
/// mutations to produce a static [`TreeViewNode`] tree. Bounded depth
/// (16) protects against pathological binding cycles.
pub(crate) fn build_tree_node(
    expr: &Expr,
    bindings: &HashMap<LocalId, Expr>,
    mutations: &HashMap<LocalId, Vec<MutationEntry>>,
    depth: usize,
) -> Option<TreeViewNode> {
    if depth > 16 {
        return None;
    }
    let (id_str, label_str, target_local) = match expr {
        Expr::LocalGet(id) => {
            let init = bindings.get(id)?;
            // The init must itself be a TreeNode(id, label) call.
            let (i, l) = treenode_id_label(init)?;
            (i, l, Some(*id))
        }
        _ => {
            let (i, l) = treenode_id_label(expr)?;
            (i, l, None)
        }
    };
    let mut children: Vec<TreeViewNode> = Vec::new();
    if let Some(lid) = target_local {
        if let Some(entries) = mutations.get(&lid) {
            for entry in entries {
                if let Mutation::TreeAddChild(child_expr) = &entry.mutation {
                    if let Some(c) = build_tree_node(child_expr, bindings, mutations, depth + 1) {
                        children.push(c);
                    }
                }
            }
        }
    }
    Some(TreeViewNode {
        id: id_str,
        label: label_str,
        children,
    })
}

/// Inspect an expr for the `TreeNode(id, label)` factory call shape and
/// extract the literal id + label strings. Returns None for any other
/// shape (unbound local, non-literal args, etc.).
pub(crate) fn treenode_id_label(expr: &Expr) -> Option<(String, String)> {
    let Expr::NativeMethodCall {
        module,
        method,
        args,
        ..
    } = expr
    else {
        return None;
    };
    if module != "perry/ui" || method != "TreeNode" {
        return None;
    }
    let id = match args.first()? {
        Expr::String(s) => s.clone(),
        _ => return None,
    };
    let label = match args.get(1)? {
        Expr::String(s) => s.clone(),
        _ => return None,
    };
    Some((id, label))
}

/// Emit ArkTS source for `chart_decls`, `tree_view_decls`, and
/// `tree_view_methods` consumed by `wrap_index_page`. Splits Chart's
/// field decls from TreeView's so the @Component layout stays readable
/// (charts come first, then tree views, then media glue).
pub(crate) fn chart_and_tree_glue(extras: &HarvestExtras) -> (String, String, String) {
    let mut chart_decls = String::new();
    for inst in &extras.chart_instances {
        chart_decls.push_str(&format!(
            "    private __chart_{id}_settings: RenderingContextSettings = new RenderingContextSettings(true);\n\
             \x20\x20\x20\x20private __chart_{id}_ctx: CanvasRenderingContext2D = new CanvasRenderingContext2D(this.__chart_{id}_settings);\n",
            id = inst.field_id,
        ));
    }

    let mut tree_view_decls = String::new();
    let mut tree_view_methods = String::new();
    for inst in &extras.tree_view_instances {
        let nodes_lit = render_tree_data_literal(&inst.root);
        tree_view_decls.push_str(&format!(
            "    private __tree_{id}_nodes: Array<{{ id: string, label: string, children: any[] }}> = [{lit}];\n\
             \x20\x20\x20\x20@State __tree_{id}_expanded: Set<string> = new Set<string>();\n\
             \x20\x20\x20\x20@State __tree_{id}_selectedId: string = '';\n",
            id = inst.field_id,
            lit = nodes_lit,
        ));
        tree_view_methods.push_str(&format!(
            "    __tree_{id}_flatten(): Array<{{ id: string, label: string, depth: number, hasChildren: boolean, expanded: boolean }}> {{\n\
             \x20\x20\x20\x20\x20\x20\x20\x20const out: Array<{{ id: string, label: string, depth: number, hasChildren: boolean, expanded: boolean }}> = [];\n\
             \x20\x20\x20\x20\x20\x20\x20\x20const expanded = this.__tree_{id}_expanded;\n\
             \x20\x20\x20\x20\x20\x20\x20\x20const walk = (nodes: Array<{{ id: string, label: string, children: any[] }}>, depth: number): void => {{\n\
             \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20for (const n of nodes) {{\n\
             \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20const has = n.children && n.children.length > 0;\n\
             \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20const isOpen = expanded.has(n.id);\n\
             \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20out.push({{ id: n.id, label: n.label, depth: depth, hasChildren: has, expanded: isOpen }});\n\
             \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20if (has && isOpen) {{ walk(n.children, depth + 1); }}\n\
             \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20}}\n\
             \x20\x20\x20\x20\x20\x20\x20\x20}};\n\
             \x20\x20\x20\x20\x20\x20\x20\x20walk(this.__tree_{id}_nodes, 0);\n\
             \x20\x20\x20\x20\x20\x20\x20\x20return out;\n\
             \x20\x20\x20\x20}}\n",
            id = inst.field_id,
        ));
    }

    (chart_decls, tree_view_decls, tree_view_methods)
}

/// Render a `TreeViewNode` (and its children, recursively) as an ArkTS
/// object-literal entry. Used by `chart_and_tree_glue` to bake the
/// static graph into the @Component's `__tree_<n>_nodes` field.
pub(crate) fn render_tree_data_literal(node: &TreeViewNode) -> String {
    let children = node
        .children
        .iter()
        .map(render_tree_data_literal)
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{{ id: {id}, label: {label}, children: [{kids}] }}",
        id = arkts_string_lit(&node.id),
        label = arkts_string_lit(&node.label),
        kids = children,
    )
}
