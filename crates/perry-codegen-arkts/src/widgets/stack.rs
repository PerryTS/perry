// This module is part of the perry-codegen-arkts crate. It was
// mechanically split out of the former monolithic lib.rs (issue
// #1100). Pure code move — no logic changes.
#![allow(clippy::too_many_arguments)]
use crate::*;

/// VStack/HStack: detect (Array, ...) vs (Number, Array, ...) signatures.
/// Recurse into the children array via `emit_widget`. Spacing prop
/// becomes `Column({space: <n>})` / `Row({space: <n>})`. ArkUI's default
/// of 0 makes spacing-less stacks look cramped, so we default to 8 which
/// matches the perry-ui-macos default.
///
/// Issue #408 — when `local_hint` is set and `mutations` has recorded
/// AddChild / ClearChildren entries against this widget, the recorded
/// children are appended after the explicit children list (and ClearChildren
/// from the mutator side drops earlier explicit children too). Conditional
/// children become `if (cond) { ChildA() } else { ChildB() }` blocks.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_stack(
    arkui_kind: &str,
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
    // First-arg shape detection — same logic as lower_call/native.rs:91.
    let (spacing, children_idx) = match args.first() {
        Some(Expr::Array(_)) | Some(Expr::ArrayMap { .. }) => (8.0, 0),
        Some(Expr::Number(n)) => (*n, 1),
        Some(Expr::Integer(n)) => (*n as f64, 1),
        _ => (8.0, 0),
    };

    let mut children = match args.get(children_idx) {
        Some(Expr::Array(items)) => items
            .iter()
            .map(|child| {
                emit_widget(
                    child,
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
            .collect::<Vec<_>>(),
        // Phase 2 v5: stack(items.map(item => Widget)) — the children
        // arg IS the array.map. Emit a single ForEach as the only child
        // of the Column/Row.
        Some(am @ Expr::ArrayMap { .. }) => vec![emit_widget(
            am,
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
        )],
        Some(_) => vec![format!(
            "// children arg wasn't an array literal — Phase 2 v1.5 limitation\n\
             Text('[non-array children]').fontSize(16).fontColor('#888888')"
        )],
        None => vec![],
    };

    // Issue #408 — fold AddChild + ClearChildren mutations.
    if let Some(id) = local_hint {
        if let Some(muts) = mutations.get(&id) {
            // ClearChildren at the unconditional level wipes the explicit
            // children list emitted from the constructor's `Array(children)`.
            // We approximate this by checking the mutation list for any
            // unconditional ClearChildren — if found, drop existing children.
            let has_unconditional_clear = muts
                .iter()
                .any(|e| matches!(e.mutation, Mutation::ClearChildren) && e.condition.is_none());
            if has_unconditional_clear {
                children.clear();
            }
            let extra = emit_mutation_children(
                muts,
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
            );
            children.extend(extra);
        }
    }

    let inner_indent = "    ".repeat(depth + 1);
    let outer_indent = "    ".repeat(depth);

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
        "{kind}({{ space: {space} }}) {{\n{body}\n{outer}}}",
        kind = arkui_kind,
        space = fmt_num(spacing),
        body = body,
        outer = outer_indent,
    )
}
