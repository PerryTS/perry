// This module is part of the perry-codegen-arkts crate. It was
// mechanically split out of the former monolithic lib.rs (issue
// #1100). Pure code move — no logic changes.
#![allow(clippy::too_many_arguments)]
use crate::*;

/// `Section(title, children)` → labeled vertical group.
/// Emits `Column({space: 4}) { Text('<title>').fontSize(14).fontColor('#888888'); <children> }`.
/// The greyed-out small label header matches the iOS UITableView section
/// header convention; no native ArkUI primitive maps 1:1, so we hand-roll.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_section(
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
    let title = first_string_arg(args).unwrap_or_default();

    let inner_indent = "    ".repeat(depth + 1);
    let outer_indent = "    ".repeat(depth);

    let mut children: Vec<String> = match args.get(1) {
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
        _ => vec![],
    };

    // Issue #408 — fold AddChild + ClearChildren mutations.
    if let Some(id) = local_hint {
        if let Some(muts) = mutations.get(&id) {
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

    // Always emit the title Text at the top, regardless of children count.
    let title_line = format!(
        "{}Text({}).fontSize(14).fontColor('#888888')",
        inner_indent,
        arkts_string_lit(&title)
    );

    let body = if children.is_empty() {
        title_line
    } else {
        let kids = children
            .iter()
            .map(|c| {
                c.lines()
                    .map(|line| format!("{}{}", inner_indent, line))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!("{}\n{}", title_line, kids)
    };

    format!(
        "Column({{ space: 4 }}) {{\n\
         {body}\n\
         {outer}}}",
        body = body,
        outer = outer_indent,
    )
}

/// Wrap a widget body expression in a complete ArkUI `@Entry @Component
// ----- Phase 2 v12 widgets -----

/// `Tabs([{label: "A", body: ...}, {label: "B", body: ...}])` →
/// ArkUI `Tabs() { TabContent() {...}.tabBar('A'); TabContent() {...}.tabBar('B') }`.
/// Each tab's body harvests like a normal sub-widget tree. Closure-bearing
/// children compose with the v2 callback registry transparently.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_tabs(
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
    let tab_specs: Vec<&Expr> = match args.first() {
        Some(Expr::Array(items)) => items.iter().collect(),
        _ => Vec::new(),
    };
    let inner_indent = "    ".repeat(depth + 1);
    let outer_indent = "    ".repeat(depth);
    let tab_blocks: Vec<String> = tab_specs
        .iter()
        .map(|spec| {
            // Each spec is `{label: string, body: Widget}`. Handle both
            // open Object and closed-shape New, same pattern as styles.
            let pairs: Option<Vec<(String, Expr)>> = match spec {
                Expr::Object(props) => Some(props.clone()),
                Expr::New {
                    class_name, args, ..
                } if class_name.starts_with("__AnonShape_") => {
                    classes.iter().find(|c| &c.name == class_name).map(|cls| {
                        cls.fields
                            .iter()
                            .enumerate()
                            .filter_map(|(i, f)| args.get(i).map(|a| (f.name.clone(), a.clone())))
                            .collect()
                    })
                }
                _ => None,
            };
            let Some(pairs) = pairs else {
                return format!(
                    "{ind}// tab spec wasn't an object\n\
                     {ind}TabContent() {{\n\
                     {ind}    Text('[invalid tab]').fontSize(16)\n\
                     {ind}}}.tabBar('?')",
                    ind = inner_indent
                );
            };
            let label = pairs
                .iter()
                .find(|(k, _)| k == "label")
                .and_then(|(_, v)| match v {
                    Expr::String(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "Tab".to_string());
            let body = pairs
                .iter()
                .find(|(k, _)| k == "body")
                .map(|(_, v)| {
                    emit_widget(
                        v,
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
                .unwrap_or_else(|| "Text('[empty tab]').fontSize(16)".to_string());
            // Indent the body inside TabContent { ... }.
            let body_indent = "    ".repeat(depth + 2);
            let body_indented = body
                .lines()
                .map(|l| format!("{}{}", body_indent, l))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "{ind}TabContent() {{\n\
                 {body}\n\
                 {ind}}}.tabBar({lbl})",
                ind = inner_indent,
                body = body_indented,
                lbl = arkts_string_lit(&label),
            )
        })
        .collect();
    let body = tab_blocks.join("\n");
    format!(
        "Tabs() {{\n\
         {body}\n\
         {outer}}}",
        body = body,
        outer = outer_indent,
    )
}

/// `Modal(title, body, [{label, action}])` → emits a small wrapper widget.
/// Real ArkUI `AlertDialog.show({...})` is fired imperatively; harvest-time
/// emission can only stage the dialog config. Phase 2 v12 emits a
/// placeholder Text + comment documenting the runtime-side wiring (a
/// proper `showDialog(...)` runtime FFI is the v12.5 follow-up).
pub(crate) fn emit_modal(_args: &[Expr], _callbacks: &mut Vec<Expr>) -> String {
    "// Modal: configure with `showDialog(...)` from a closure body \
     (Phase 2 v12.5 — needs runtime FFI bridge to AlertDialog.show)\n\
     Text('[Modal — call showDialog() instead]').fontSize(16).fontColor('#888888')"
        .to_string()
}

/// `Menu([{label, action}])` → ArkUI menu shape. ArkUI's `.bindMenu(...)` is
/// a modifier on a triggering widget, not a standalone widget. Phase 2 v12
/// emits the menu as a `Column { Button(label) }` for each item — visible
/// + functional via the v2 callback registry — and the user can wrap it
/// in any container they want. Real `.bindMenu()` modifier integration is
/// v12.5.
pub(crate) fn emit_menu(args: &[Expr], callbacks: &mut Vec<Expr>) -> String {
    let items: Vec<&Expr> = match args.first() {
        Some(Expr::Array(items)) => items.iter().collect(),
        _ => Vec::new(),
    };
    let buttons: Vec<String> = items
        .iter()
        .map(|item| {
            let pairs: Option<Vec<(String, Expr)>> = match item {
                Expr::Object(props) => Some(props.clone()),
                _ => None,
            };
            let Some(pairs) = pairs else {
                return "Text('[invalid menu item]').fontSize(14).fontColor('#888888')".to_string();
            };
            let label = pairs
                .iter()
                .find(|(k, _)| k == "label")
                .and_then(|(_, v)| match v {
                    Expr::String(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "Item".to_string());
            let action = pairs.iter().find(|(k, _)| k == "action").map(|(_, v)| v);
            // Reuse Button's emit shape so action closures register
            // correctly via the v2 callback pipeline.
            let pseudo_args: Vec<Expr> = vec![
                Expr::String(label.clone()),
                action.cloned().unwrap_or(Expr::Number(0.0)),
            ];
            emit_button(&pseudo_args, callbacks)
        })
        .collect();
    format!(
        "Column({{ space: 4 }}) {{\n    {}\n}}",
        buttons.join("\n    "),
    )
}

/// `Grid(columns, items)` → ArkUI `Grid() { GridItem() {...} }` with
/// `.columnsTemplate('1fr 1fr ...')` for the column count.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_grid(
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
    let columns = numeric_arg(args, 0).unwrap_or(2.0) as i64;
    let columns = columns.clamp(1, 12);
    let template = (0..columns).map(|_| "1fr").collect::<Vec<_>>().join(" ");
    let items: Vec<&Expr> = match args.get(1) {
        Some(Expr::Array(items)) => items.iter().collect(),
        _ => Vec::new(),
    };
    let inner_indent = "    ".repeat(depth + 1);
    let outer_indent = "    ".repeat(depth);
    let grid_items: Vec<String> = items
        .iter()
        .map(|child| {
            let body = emit_widget(
                child,
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
            );
            let body_indent = "    ".repeat(depth + 2);
            let body_indented = body
                .lines()
                .map(|l| format!("{}{}", body_indent, l))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "{ind}GridItem() {{\n{body}\n{ind}}}",
                ind = inner_indent,
                body = body_indented,
            )
        })
        .collect();
    format!(
        "Grid() {{\n\
         {body}\n\
         {outer}}}.columnsTemplate('{template}')",
        body = grid_items.join("\n"),
        outer = outer_indent,
        template = template,
    )
}

/// Phase 2 v11: `NavStack(state, [{name, body}, ...])` for multi-page
/// navigation. Composes on the v6 state<T> + v3.2 reactive-Text bridge
/// instead of ArkUI's heavier `Navigation` + `NavPathStack` + @Builder
/// pattern — the user holds a `state<string>("home")` for the active
/// route, and `route.set("detail")` from any closure flips the visible
/// branch via the existing setText drain queue. Zero new runtime FFIs.
///
/// Native ArkUI back-gesture integration (proper `Navigation` +
/// `NavDestination` + `pageStack.pop()` on Android-style hardware-back)
/// is the v11.5 follow-up — it needs the @Builder-based pattern that
/// requires real navigator state on the page struct, not a string state.
/// The state-driven if/elseif emission shipped here covers the canonical
/// "tap button → forward; tap back button → state.set(prev)" happy
/// path, which is what most apps actually need.
///
/// Emit shape:
/// ```ets
/// Column() {
///     if (this.text_<sid> === 'home') {
///         <home body>
///     } else if (this.text_<sid> === 'detail') {
///         <detail body>
///     }
/// }
/// ```
///
/// `args[0]` must be `Expr::LocalGet(state_id)` referring to a
/// state<string> binding harvested by `collect_state_bindings`. If it's
/// not, emit a placeholder comment + use the first route as fallback so
/// the page still renders something.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_nav_stack(
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

    // Resolve the state arg — must be a LocalGet whose id is registered
    // in state_registry (v6 collect_state_bindings handles this on init).
    // Register the synth_id with a text_slot so wrap_index_page emits
    // the @State decl + applyTextUpdate dispatch arm.
    let state_field = match args.first() {
        Some(Expr::LocalGet(id)) => state_registry.get(id).map(|b| {
            let field_id = sanitize_text_id(&b.synth_id);
            // Avoid double-registering if the user *also* called
            // route.text() somewhere else in the tree — text_slots is
            // de-duped by original_id at wrap_index_page emission time.
            text_slots.push(TextSlot {
                original_id: b.synth_id.clone(),
                field_id: field_id.clone(),
                initial: b.initial_str.clone(),
            });
            field_id
        }),
        _ => None,
    };

    // Routes array: each elem is `{name: string, body: Widget}` (open
    // Object) or `__AnonShape_*` New (Perry's closed-shape form).
    let route_specs: Vec<&Expr> = match args.get(1) {
        Some(Expr::Array(items)) => items.iter().collect(),
        _ => Vec::new(),
    };

    if route_specs.is_empty() {
        return format!(
            "Column() {{\n\
             {ind}// NavStack: empty routes array\n\
             {outer}}}",
            ind = inner_indent,
            outer = outer_indent,
        );
    }

    // No state binding — fall back to rendering only the first route so
    // the page still has something visible. Emit a developer-facing hint
    // comment so the lapse is discoverable.
    let Some(state_field) = state_field else {
        let first_body = extract_route_body(route_specs[0], classes)
            .map(|body| {
                emit_widget(
                    &body,
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
            .unwrap_or_else(|| "Text('[invalid route body]').fontSize(16)".to_string());
        let body_indent = "    ".repeat(depth + 1);
        let first_body_indented = first_body
            .lines()
            .map(|l| format!("{}{}", body_indent, l))
            .collect::<Vec<_>>()
            .join("\n");
        return format!(
            "Column() {{\n\
             {ind}// NavStack: first arg must be a `state<string>(...)` local — \
             rendering first route only\n\
             {body}\n\
             {outer}}}",
            ind = inner_indent,
            body = first_body_indented,
            outer = outer_indent,
        );
    };

    // Per-route emission: each gets an `if/else if` arm keyed on the
    // state field's current value. The first route is the `if`; the rest
    // are `else if`. We don't add a final `else` — if the state holds an
    // unknown route name, nothing renders, which is the expected
    // behavior for a cleared/unset route.
    let mut arms: Vec<String> = Vec::new();
    for (idx, spec) in route_specs.iter().enumerate() {
        let name = extract_route_name(spec, classes).unwrap_or_else(|| format!("route_{}", idx));
        let body_expr = extract_route_body(spec, classes);
        let body_str = body_expr
            .as_ref()
            .map(|b| {
                emit_widget(
                    b,
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
            .unwrap_or_else(|| "Text('[empty route]').fontSize(16)".to_string());
        let body_indent = "    ".repeat(depth + 2);
        let body_indented = body_str
            .lines()
            .map(|l| format!("{}{}", body_indent, l))
            .collect::<Vec<_>>()
            .join("\n");
        let keyword = if idx == 0 { "if" } else { "else if" };
        arms.push(format!(
            "{ind}{kw} (this.text_{field} === {lit}) {{\n\
             {body}\n\
             {ind}}}",
            ind = inner_indent,
            kw = keyword,
            field = state_field,
            lit = arkts_string_lit(&name),
            body = body_indented,
        ));
    }

    format!(
        "Column() {{\n\
         {body}\n\
         {outer}}}",
        body = arms.join(" "),
        outer = outer_indent,
    )
}

/// struct Index { build() { Column() { ... } } }` page.
///
/// The leading imports make `perryEntry.invokeCallback` (Phase 2 v2),
/// `perryEntry.drainToast` + `promptAction.showToast` (v3 Option 1),
/// and `perryEntry.drainTextUpdate` (v3 Option 2) available to the
/// auto-emitted `.onClick(...)` handlers.
///
/// `text_slots` is the list of reactive `Text(content, id)` registrations
/// collected during the widget walk. For each slot we emit:
///   - `@State text_<id>: string = '<initial>'` field decl
///   - a switch arm in `applyTextUpdate(id, value)` that assigns to
///     the matching field
pub(crate) fn wrap_index_page(
    widget_body: &str,
    text_slots: &[TextSlot],
    lazy_sources: &[LazyDataSource],
    uses_media: bool,
    visibility_bindings: &HashMap<LocalId, VisibilityBinding>,
    view_builders: &[ViewBuilder],
    extras: &HarvestExtras,
) -> String {
    let indented = widget_body
        .lines()
        .map(|line| format!("            {}", line))
        .collect::<Vec<_>>()
        .join("\n");

    // @State decls (one per registered reactive Text). Field names use
    // the sanitized id; literals come straight from the user's TS.
    let state_decls: String = text_slots
        .iter()
        .map(|slot| {
            format!(
                "    @State text_{}: string = {};\n",
                slot.field_id,
                arkts_string_lit(&slot.initial)
            )
        })
        .collect();

    // Phase 2 v10 — `@State <id>: PerryListDataSource = new PerryListDataSource(<items>)`
    // for each LazyVStack(items.map(...)) in the harvested tree.
    let lazy_decls: String = lazy_sources
        .iter()
        .map(|src| {
            format!(
                "    @State {}: PerryListDataSource = new PerryListDataSource({});\n",
                src.field_id, src.items_source,
            )
        })
        .collect();

    // Phase 2 v10 — boilerplate IDataSource class. Emitted once per page
    // if any LazyVStack registered a source. Idempotent (no-op if none).
    let lazy_class = if lazy_sources.is_empty() {
        String::new()
    } else {
        "\
class PerryListDataSource implements IDataSource {\n\
    private items: any[];\n\
    private listeners: DataChangeListener[] = [];\n\
    constructor(items: any[]) { this.items = items; }\n\
    totalCount(): number { return this.items.length; }\n\
    getData(idx: number): any { return this.items[idx]; }\n\
    registerDataChangeListener(listener: DataChangeListener): void { this.listeners.push(listener); }\n\
    unregisterDataChangeListener(listener: DataChangeListener): void { this.listeners = this.listeners.filter(l => l !== listener); }\n\
}\n\n"
            .to_string()
    };

    // applyTextUpdate(id, value) switch arms. Always emit the method,
    // even with zero slots, so the auto-generated onClick body's call
    // resolves at ArkTS compile time. The switch matches the ORIGINAL
    // id (what the runtime queues from `setText("user-name", ...)`)
    // and assigns to the SANITIZED field name.
    let switch_arms: String = text_slots
        .iter()
        .map(|slot| {
            format!(
                "            case {}: this.text_{} = value; break;\n",
                arkts_string_lit(&slot.original_id),
                slot.field_id
            )
        })
        .collect();
    let apply_method = format!(
        "    applyTextUpdate(id: string, value: string): void {{\n\
         \x20\x20\x20\x20\x20\x20\x20\x20switch (id) {{\n\
         {arms}\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20default: break;\n\
         \x20\x20\x20\x20\x20\x20\x20\x20}}\n\
         \x20\x20\x20\x20}}\n",
        arms = switch_arms
    );

    // Phase 2 v3.5 — `@State hidden_<id>: boolean = <initial>` declarations
    // and applyVisibilityUpdate switch method. Iteration order is the
    // BTreeSet ordering from `collect_visibility_bindings` so the emitted
    // bytes are stable across re-runs.
    let mut sorted_visibility: Vec<(&LocalId, &VisibilityBinding)> =
        visibility_bindings.iter().collect();
    sorted_visibility.sort_by_key(|(id, _)| **id);
    let visibility_decls: String = sorted_visibility
        .iter()
        .map(|(_, binding)| {
            format!(
                "    @State hidden_{}: boolean = {};\n",
                binding.synth_id, binding.initial_hidden
            )
        })
        .collect();
    let visibility_arms: String = sorted_visibility
        .iter()
        .map(|(_, binding)| {
            format!(
                "            case {}: this.hidden_{} = hidden; break;\n",
                arkts_string_lit(&binding.synth_id),
                binding.synth_id
            )
        })
        .collect();
    let apply_visibility_method = format!(
        "    applyVisibilityUpdate(id: string, hidden: boolean): void {{\n\
         \x20\x20\x20\x20\x20\x20\x20\x20switch (id) {{\n\
         {arms}\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20default: break;\n\
         \x20\x20\x20\x20\x20\x20\x20\x20}}\n\
         \x20\x20\x20\x20}}\n",
        arms = visibility_arms
    );

    // Phase 2 v3.6 — `@State contentView_<target_synth>: string = 'default'`
    // declarations. One per UNIQUE target_synth (multiple view-builders
    // for the same target share the same @State). 'default' as the empty
    // initial value matches the `if (this.contentView_X === 'Y')`
    // condition: at startup, no view is active, so the lifted branches
    // don't render — the unconditional default content from module init
    // shows.
    let mut content_view_targets: std::collections::BTreeMap<String, ()> =
        std::collections::BTreeMap::new();
    for b in view_builders {
        content_view_targets.insert(b.target_synth.clone(), ());
    }
    let content_view_decls: String = content_view_targets
        .keys()
        .map(|target_synth| {
            format!(
                "    @State contentView_{}: string = 'default';\n",
                target_synth
            )
        })
        .collect();
    // applyContentViewUpdate switch: matches by target_synth and
    // assigns view_id to the corresponding @State field. Always emit
    // the method even with zero builders so the auto-emitted onClick
    // body's call resolves at ArkTS compile time.
    let mut sorted_targets: Vec<&String> = content_view_targets.keys().collect();
    sorted_targets.sort();
    let content_view_arms: String = sorted_targets
        .iter()
        .map(|target_synth| {
            format!(
                "            case {}: this.contentView_{} = view; break;\n",
                arkts_string_lit(target_synth),
                target_synth
            )
        })
        .collect();
    let apply_content_view_method = format!(
        "    applyContentViewUpdate(id: string, view: string): void {{\n\
         \x20\x20\x20\x20\x20\x20\x20\x20switch (id) {{\n\
         {arms}\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20default: break;\n\
         \x20\x20\x20\x20\x20\x20\x20\x20}}\n\
         \x20\x20\x20\x20}}\n",
        arms = content_view_arms
    );

    // Issue #369 — perry/media drain glue. Emitted only when the harvest
    // walker saw any `perry/media` NativeMethodCall in the module. The
    // pump runs on the ArkTS UI thread (setInterval is bound to the
    // current ability's run loop), so the AVPlayer dispatches and the
    // pushMediaState callback land on the same thread Perry's main()
    // runs on — closures fired from `media_playback::push_media_state`
    // can safely allocate into the per-thread arena.
    let (media_imports, media_decls, media_methods, media_pump) = if uses_media {
        media_glue()
    } else {
        (String::new(), String::new(), String::new(), String::new())
    };

    // Issue #669 / #670 — Chart + TreeView class-level state.
    let (chart_decls, tree_view_decls, tree_view_methods) = chart_and_tree_glue(extras);

    format!(
        "// Auto-generated by Perry (perry-codegen-arkts) — do not edit.\n\
         // Regenerated every `perry compile --target harmonyos`.\n\
         //\n\
         // Source of truth is the `App({{body: ...}})` call in your\n\
         // TypeScript entry. Edit there; this file is overwritten.\n\
         import perryEntry from 'libentry.so';\n\
         import promptAction from '@ohos.promptAction';\n\
         {media_imports}\
         \n\
         {lazy_class}\
         @Entry\n\
         @Component\n\
         struct Index {{\n\
         {states}\
         {visibility_decls}\
         {content_view_decls}\
         {lazy_decls}\
         {chart_decls}\
         {tree_view_decls}\
         {media_decls}\
         {apply}\
         {apply_visibility}\
         {apply_content_view}\
         {tree_view_methods}\
         {media_methods}\
         \x20\x20\x20\x20build() {{\n\
         \x20\x20\x20\x20\x20\x20\x20\x20Column() {{\n\
         {body}\n\
         \x20\x20\x20\x20\x20\x20\x20\x20}}\n\
         \x20\x20\x20\x20\x20\x20\x20\x20.width('100%')\n\
         \x20\x20\x20\x20\x20\x20\x20\x20.height('100%')\n\
         \x20\x20\x20\x20\x20\x20\x20\x20.justifyContent(FlexAlign.Center)\n\
         {media_pump}\
         \x20\x20\x20\x20}}\n\
         }}\n",
        states = state_decls,
        visibility_decls = visibility_decls,
        content_view_decls = content_view_decls,
        lazy_class = lazy_class,
        lazy_decls = lazy_decls,
        chart_decls = chart_decls,
        tree_view_decls = tree_view_decls,
        tree_view_methods = tree_view_methods,
        apply = apply_method,
        apply_visibility = apply_visibility_method,
        apply_content_view = apply_content_view_method,
        body = indented,
        media_imports = media_imports,
        media_decls = media_decls,
        media_methods = media_methods,
        media_pump = media_pump,
    )
}
