// This module is part of the perry-codegen-arkts crate. It was
// mechanically split out of the former monolithic lib.rs (issue
// #1100). Pure code move — no logic changes.
#![allow(clippy::too_many_arguments)]
use crate::*;

/// `ScrollView(children)` → `Scroll() { Column({space: 8}) { ... } }`.
/// ArkUI's `Scroll` is a single-child container that scrolls vertically by
/// default; we wrap in a `Column` so multiple children stack the way users
/// expect from the perry-ui-* native ScrollView wiring. Empty / non-array
/// children degrade to an empty Scroll just like the native variant.
///
/// Issue #408 — when `local_hint` resolves to a recorded set of mutations
/// against this scroll local, `scrollviewSetChild(scroll, content)` calls
/// inject the content as a child of the inner Column (latest one wins),
/// and `widgetAddChild(scroll, child)` calls also append into the inner
/// Column.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_scrollview(
    args: &[Expr],
    bindings: &HashMap<LocalId, Expr>,
    depth: usize,
    callbacks: &mut Vec<Expr>,
    text_slots: &mut Vec<TextSlot>,
    arkts_locals: &HashMap<LocalId, String>,
    classes: &[Class],
    state_registry: &HashMap<LocalId, StateBinding>,
    lazy_sources: &mut Vec<LazyDataSource>,
    extras: &mut HarvestExtras,
    mutations: &HashMap<LocalId, Vec<MutationEntry>>,
    local_hint: Option<LocalId>,
) -> String {
    let inner_indent = "    ".repeat(depth + 2);
    let mid_indent = "    ".repeat(depth + 1);
    let outer_indent = "    ".repeat(depth);

    let mut children: Vec<String> = match args.first() {
        Some(Expr::Array(items)) => items
            .iter()
            .map(|c| {
                emit_widget(
                    c,
                    bindings,
                    depth + 2,
                    callbacks,
                    text_slots,
                    arkts_locals,
                    classes,
                    state_registry,
                    lazy_sources,
                    extras,
                    mutations,
                    None,
                )
            })
            .collect(),
        Some(am @ Expr::ArrayMap { .. }) => vec![emit_widget(
            am,
            bindings,
            depth + 2,
            callbacks,
            text_slots,
            arkts_locals,
            classes,
            state_registry,
            lazy_sources,
            extras,
            mutations,
            None,
        )],
        _ => vec![],
    };

    // Issue #408 — fold scroll-specific mutations.
    // SetScrollChild semantics: latest wins, replaces ALL prior children
    // (matches the native `scrollviewSetChild` behavior). AddChild on a
    // ScrollView is rare but supported — appends inside the inner Column.
    if let Some(id) = local_hint {
        if let Some(muts) = mutations.get(&id) {
            // Find the LAST unconditional SetScrollChild — that wins.
            let last_set = muts.iter().rposition(|e| {
                matches!(e.mutation, Mutation::SetScrollChild(_)) && e.condition.is_none()
            });
            if let Some(idx) = last_set {
                if let Mutation::SetScrollChild(content) = &muts[idx].mutation {
                    children = vec![emit_widget(
                        content,
                        bindings,
                        depth + 2,
                        callbacks,
                        text_slots,
                        arkts_locals,
                        classes,
                        state_registry,
                        lazy_sources,
                        extras,
                        mutations,
                        None,
                    )];
                }
            }
            // Append AddChild + conditional groups (built from BOTH AddChild
            // and SetScrollChild — the latter is essentially "replace + add").
            // For conditional SetScrollChild, treat each set as a single-child
            // override INSIDE its branch: we synthesize an AddChild-style
            // entry so emit_mutation_children can render it as an `if` block.
            let synthesized: Vec<MutationEntry> = muts
                .iter()
                .filter_map(|e| match (&e.mutation, &e.condition) {
                    (Mutation::AddChild(_), _) => Some(e.clone()),
                    (Mutation::ClearChildren, _) => Some(e.clone()),
                    (Mutation::SetScrollChild(c), Some(_)) => Some(MutationEntry {
                        mutation: Mutation::AddChild(c.clone()),
                        condition: e.condition.clone(),
                    }),
                    _ => None,
                })
                .collect();
            let extra = emit_mutation_children(
                &synthesized,
                bindings,
                depth + 2,
                callbacks,
                text_slots,
                arkts_locals,
                classes,
                state_registry,
                lazy_sources,
                extras,
                mutations,
            );
            children.extend(extra);
        }
    }

    let body = if children.is_empty() {
        String::new()
    } else {
        children
            .iter()
            .map(|c| {
                c.lines()
                    .map(|line| format!("{}{}", inner_indent, line))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "Scroll() {{\n\
         {mid}Column({{ space: 8 }}) {{\n\
         {body}\n\
         {mid}}}\n\
         {outer}}}",
        mid = mid_indent,
        body = body,
        outer = outer_indent,
    )
}

/// `LazyVStack(children)` → for now just emit `Column({space: 8}) { ... }`.
/// Real lazy rendering needs ArkUI's `LazyForEach` + a custom `IDataSource`
/// implementation, which doesn't fit the static-tree harvest model — the
/// children would have to be a function `(index) => Widget` evaluated per
/// row, which isn't expressible in the harvest pass without a runtime
/// callback bridge. Deferred to a future Phase 2 v5; today users write the
/// expanded children list explicitly and pay the eager-render cost.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_lazy_vstack(
    args: &[Expr],
    bindings: &HashMap<LocalId, Expr>,
    depth: usize,
    callbacks: &mut Vec<Expr>,
    text_slots: &mut Vec<TextSlot>,
    arkts_locals: &HashMap<LocalId, String>,
    classes: &[Class],
    state_registry: &HashMap<LocalId, StateBinding>,
    lazy_sources: &mut Vec<LazyDataSource>,
    extras: &mut HarvestExtras,
    mutations: &HashMap<LocalId, Vec<MutationEntry>>,
) -> String {
    let inner_indent = "    ".repeat(depth + 1);
    let outer_indent = "    ".repeat(depth);

    // Phase 2 v10 — Real LazyVStack: when args[0] is `Expr::ArrayMap`,
    // emit ArkUI's `List() { LazyForEach(this.<src>, item => { ListItem() {<inner>} }, item => item) }`
    // and register a `PerryListDataSource`-backed `@State` field on the
    // page struct. wrap_index_page emits the IDataSource helper class +
    // the per-source field decls.
    if let Some(Expr::ArrayMap { array, callback }) = args.first() {
        let items_source = arkts_array_source(array, bindings);
        let field_id = format!("lazy_source_{}", lazy_sources.len());
        // Lower the closure body in a fresh arkts_locals scope so
        // LocalGet(param_id) resolves to `__item`.
        let (param_name, body_str) = match callback.as_ref() {
            Expr::Closure { params, body, .. } if !params.is_empty() => {
                let body_expr = body.iter().find_map(|s| match s {
                    Stmt::Return(Some(e)) => Some(e.clone()),
                    Stmt::Expr(e) => Some(e.clone()),
                    _ => None,
                });
                if let Some(body) = body_expr {
                    let mut locals = arkts_locals.clone();
                    locals.insert(params[0].id, "__item".to_string());
                    let inner = emit_widget(
                        &body,
                        bindings,
                        depth + 3,
                        callbacks,
                        text_slots,
                        &locals,
                        classes,
                        state_registry,
                        lazy_sources,
                        extras,
                        mutations,
                        None,
                    );
                    ("__item".to_string(), inner)
                } else {
                    (
                        "__item".to_string(),
                        "Text('[empty body]').fontSize(16)".to_string(),
                    )
                }
            }
            _ => (
                "__item".to_string(),
                "Text('[non-closure ForEach body]').fontSize(16)".to_string(),
            ),
        };
        // Push the source AFTER recursive emit_widget to maintain a
        // deterministic ordering (outermost-last so nested LazyVStacks
        // get inner ids before outer).
        lazy_sources.push(LazyDataSource {
            field_id: field_id.clone(),
            items_source,
        });
        let item_indent = "    ".repeat(depth + 3);
        let body_indented = body_str
            .lines()
            .map(|l| format!("{}{}", item_indent, l))
            .collect::<Vec<_>>()
            .join("\n");
        let mid_indent = "    ".repeat(depth + 2);
        return format!(
            "List() {{\n\
             {inner}LazyForEach(this.{field}, ({pname}: any) => {{\n\
             {mid}ListItem() {{\n\
             {body}\n\
             {mid}}}\n\
             {inner}}}, ({pname}: any) => {pname})\n\
             {outer}}}",
            inner = inner_indent,
            mid = mid_indent,
            field = field_id,
            pname = param_name,
            body = body_indented,
            outer = outer_indent,
        );
    }

    // Fall-through (v4 behavior): non-ArrayMap children render eagerly
    // as a plain Column. Preserves backwards compat for explicit-list
    // LazyVStack callers.
    let children: Vec<String> = match args.first() {
        Some(Expr::Array(items)) => items
            .iter()
            .map(|c| {
                emit_widget(
                    c,
                    bindings,
                    depth + 1,
                    callbacks,
                    text_slots,
                    arkts_locals,
                    classes,
                    state_registry,
                    lazy_sources,
                    extras,
                    mutations,
                    None,
                )
            })
            .collect(),
        _ => vec![],
    };
    let body = if children.is_empty() {
        String::new()
    } else {
        children
            .iter()
            .map(|c| {
                c.lines()
                    .map(|line| format!("{}{}", inner_indent, line))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "// LazyVStack with explicit children: rendered eagerly as Column.\n\
         {outer}// For real lazy rendering, pass `items.map(item => Widget)`.\n\
         {outer}Column({{ space: 8 }}) {{\n\
         {body}\n\
         {outer}}}",
        outer = outer_indent,
        body = body,
    )
}
