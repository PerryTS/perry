// This module is part of the perry-codegen-arkts crate. It was
// mechanically split out of the former monolithic lib.rs (issue
// #1100). Pure code move — no logic changes.
#![allow(clippy::too_many_arguments)]
use crate::*;

/// Emit an ArkUI expression for a perry/ui widget call. Returns the inner
/// `build()`-block content (no wrapping component). `depth` controls
/// indentation when emitting nested children. `callbacks` accumulates
/// closure expressions that need runtime registration; each push assigns
/// the next slot id (= callbacks.len() before push).
///
/// Unrecognized widgets degrade to a comment + a placeholder Text — never
/// errors out, since emit-time errors would leave the user without any UI.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_widget(
    expr: &Expr,
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
    // `outer_local_hint` is set when the caller already knows the
    // top-level LocalId we're emitting for — used by recursive calls
    // from emit_stack into a child position that may itself be a
    // LocalGet of another widget local. Always None at the entry point.
    outer_local_hint: Option<LocalId>,
) -> String {
    // Issue #408 — extract LocalId hint before resolving so we can later
    // look up procedural mutations recorded against this widget binding.
    // `outer_local_hint` overrides nothing: if expr is itself a LocalGet,
    // its id wins over a caller-supplied hint.
    let local_hint = match expr {
        Expr::LocalGet(id) => Some(*id),
        _ => outer_local_hint,
    };
    // Phase 2 v6 — `state.text()` shape: Expr::Call { callee: PropertyGet
    // { obj: LocalGet(state_id), property: "text" }, args: [] } where
    // state_id is in the registry. Emit a reactive Text using the
    // registered synth_id + initial value (uses the v3.2 path).
    if let Expr::Call { callee, args, .. } = expr {
        if args.is_empty() {
            if let Expr::PropertyGet {
                object, property, ..
            } = callee.as_ref()
            {
                if property == "text" {
                    if let Expr::LocalGet(state_id) = object.as_ref() {
                        if let Some(binding) = state_registry.get(state_id) {
                            text_slots.push(TextSlot {
                                original_id: binding.synth_id.clone(),
                                field_id: sanitize_text_id(&binding.synth_id),
                                initial: binding.initial_str.clone(),
                            });
                            return format!(
                                "Text(this.text_{}).fontSize(20)",
                                sanitize_text_id(&binding.synth_id)
                            );
                        }
                    }
                }
            }
        }
    }
    let resolved = resolve(expr, bindings);
    match &resolved {
        Expr::NativeMethodCall {
            module: m,
            method,
            args,
            ..
        } if m == "perry/ui" => {
            let core = match method.as_str() {
                "Text" => emit_text(args, text_slots, arkts_locals, bindings),
                "VStack" => emit_stack(
                    "Column",
                    args,
                    bindings,
                    depth,
                    callbacks,
                    text_slots,
                    arkts_locals,
                    classes,
                    state_registry,
                    lazy_sources,
                    extras,
                    mutations,
                    local_hint,
                ),
                "HStack" => emit_stack(
                    "Row",
                    args,
                    bindings,
                    depth,
                    callbacks,
                    text_slots,
                    arkts_locals,
                    classes,
                    state_registry,
                    lazy_sources,
                    extras,
                    mutations,
                    local_hint,
                ),
                "Button" => emit_button(args, callbacks),
                "TextField" => emit_textfield(args, callbacks),
                "Toggle" => emit_toggle(args, callbacks),
                "Slider" => emit_slider(args, callbacks),
                "Spacer" => "Blank()".to_string(),
                "Divider" => "Divider()".to_string(),
                "Image" | "ImageFile" => emit_image(args, bindings),
                "ScrollView" => emit_scrollview(
                    args,
                    bindings,
                    depth,
                    callbacks,
                    text_slots,
                    arkts_locals,
                    classes,
                    state_registry,
                    lazy_sources,
                    extras,
                    mutations,
                    local_hint,
                ),
                "LazyVStack" => emit_lazy_vstack(
                    args,
                    bindings,
                    depth,
                    callbacks,
                    text_slots,
                    arkts_locals,
                    classes,
                    state_registry,
                    lazy_sources,
                    extras,
                    mutations,
                ),
                "Picker" => emit_picker(args, callbacks),
                // Issue #475 — Combobox(initial, onChange) maps to ArkUI
                // Select. Runtime-added items (`comboboxAddItem`) are
                // currently not folded into the static options array;
                // the emitted Select shows the `initial` value as its
                // only option. Tracked for v1.1 follow-up.
                "Combobox" => emit_combobox(args, callbacks),
                // Issue #478 — RichTextEditor(width, height, onChange)
                // maps to ArkUI RichEditor. HTML round-trip + per-span
                // bold/italic/underline toggles are #478 v1.1.
                "RichTextEditor" => emit_richtexteditor(args, callbacks),
                // Issue #481 — Calendar(year, month, onChange) maps to
                // ArkUI's CalendarPicker. Per the #481 v1 brief, the
                // picker variant is simpler than the full Calendar
                // (which would need a builder for cell rendering).
                "Calendar" => emit_calendar(args, callbacks),
                // Issue #4772 — DatePicker(year, month, onChange) maps to
                // ArkUI's native DatePicker (compact field complement to
                // Calendar's CalendarPicker grid).
                "DatePicker" => emit_date_picker(args, callbacks),
                "ProgressView" => emit_progressview(args),
                "Section" => emit_section(
                    args,
                    bindings,
                    depth,
                    callbacks,
                    text_slots,
                    arkts_locals,
                    classes,
                    state_registry,
                    lazy_sources,
                    extras,
                    mutations,
                    local_hint,
                ),
                // Phase 2 v12 widgets.
                "Tabs" => emit_tabs(
                    args,
                    bindings,
                    depth,
                    callbacks,
                    text_slots,
                    arkts_locals,
                    classes,
                    state_registry,
                    lazy_sources,
                    extras,
                    mutations,
                ),
                "Modal" | "Dialog" => emit_modal(args, callbacks),
                "Menu" | "ContextMenu" => emit_menu(args, callbacks),
                "Grid" => emit_grid(
                    args,
                    bindings,
                    depth,
                    callbacks,
                    text_slots,
                    arkts_locals,
                    classes,
                    state_registry,
                    lazy_sources,
                    extras,
                    mutations,
                ),
                // Phase 2 v11: state-driven multi-page nav.
                "NavStack" => emit_nav_stack(
                    args,
                    bindings,
                    depth,
                    callbacks,
                    text_slots,
                    arkts_locals,
                    classes,
                    state_registry,
                    lazy_sources,
                    extras,
                    mutations,
                ),
                // Issue #669 — Chart(kind, w, h) → ArkUI Canvas with the
                // mutator-folded data baked into the draw closure.
                "Chart" => emit_chart(args, bindings, mutations, local_hint, extras),
                // Issue #670 — TreeView(root, onSelect) → ArkUI List with
                // a recursive flatten over a static node graph + @State
                // expanded/selected fields.
                "TreeView" => emit_treeview(args, bindings, callbacks, mutations, extras),
                // Issue #670 — TreeNode(id, label) is a node-graph
                // builder, not a renderable widget. Its only legal
                // appearance in the widget tree is as a TreeView's
                // first arg (handled above). Falling here means the
                // user used TreeNode in an unsupported position —
                // emit a comment + placeholder so the build still works.
                "TreeNode" => "// TreeNode used outside TreeView's first arg — not renderable\n\
                     Text('[TreeNode misplaced]').fontSize(14).fontColor('#888888')"
                    .to_string(),
                other => format!(
                    "// unsupported perry/ui widget: {} (Phase 2 v12)\n\
                     Text('[unsupported: {}]').fontSize(16).fontColor('#888888')",
                    other, other
                ),
            };
            // Phase 2 v5: detect a trailing StyleProps object and append
            // its modifier chain. Disambiguates Text's 2nd-arg id-vs-style
            // by checking whether the last arg is an object (style) or a
            // plain string (id) — Text("hi", "id") leaves args.last() as
            // a String which extract_style_object returns None for.
            let style_props = args.last().and_then(|a| extract_style_object(a, classes));
            let mut out = if let Some(props) = style_props {
                let modifiers = emit_style_modifiers(&props);
                if !modifiers.is_empty() {
                    format!("{}{}", core, modifiers)
                } else {
                    core
                }
            } else {
                core
            };
            // Issue #408 — append modifier mutations recorded against this
            // widget local. Stack/ScrollView/Section emitters fold AddChild
            // / SetScrollChild / ClearChildren mutations into their bodies
            // directly (see those functions); Modifier mutations are
            // append-only and apply to *every* widget kind so we handle
            // them here unconditionally.
            if let Some(id) = local_hint {
                if let Some(muts) = mutations.get(&id) {
                    out.push_str(&emit_modifier_mutations(muts));
                }
            }
            out
        }
        // Phase 2 v5: ForEach via array.map. When a widget position
        // contains `array.map(item => widgetExpr)`, lower it to ArkUI's
        // ForEach with the closure body emitted in a fresh local-scope
        // env where the closure's param resolves to `__item`.
        Expr::ArrayMap { array, callback } => emit_for_each(
            array,
            callback,
            bindings,
            depth,
            callbacks,
            text_slots,
            arkts_locals,
            classes,
            state_registry,
            lazy_sources,
            extras,
            mutations,
        ),
        // Issue #408 follow-up — ternary `cond ? thenWidget : elseWidget`
        // (HIR `Expr::Conditional`). Mango's pattern:
        //
        //     const toolbarRow = mobile
        //       ? HStack(10, [...mobileChildren])
        //       : HStack(10, [...desktopChildren]);
        //
        // Try to const-fold the condition first — if it resolves to a
        // literal bool, emit the corresponding branch unconditionally.
        // If the condition involves runtime values (function calls,
        // unresolved props), we can't reliably pick — default to the
        // then-branch (the heuristic that picks the "primary" / first-
        // listed case, matching what users typically write first).
        // Without this arm Conditional widget refs fell through to
        // `[unrecognized body]` even though both branches are real
        // widget calls the harvest CAN emit.
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            let folded = evaluate_condition(condition, bindings, &HashMap::new());
            let chosen = match folded {
                Some(false) => else_expr,
                _ => then_expr, // true OR unresolved → take the then-branch
            };
            emit_widget(
                chosen,
                bindings,
                depth,
                callbacks,
                text_slots,
                arkts_locals,
                classes,
                state_registry,
                lazy_sources,
                extras,
                mutations,
                local_hint,
            )
        }
        _ => "// unrecognized body expression (must be a perry/ui widget call)\n\
             Text('[unrecognized body]').fontSize(16).fontColor('#888888')"
            .to_string(),
    }
}
