//! ArkUI/ArkTS code generation for Perry --target harmonyos.
//!
//! HarmonyOS NEXT renders UI declaratively from `.ets` files annotated with
//! `@Entry @Component struct ... { build() { ... } }`. Perry's `perry/ui`
//! surface (`App({body: VStack([Text("hi"), Button("OK", () => {})])})`) is
//! normally lowered to native FFI calls (perry_ui_*_create / set_*) on
//! iOS / macOS / Android / Linux / Windows — backed by perry-ui-* crates that
//! call into UIKit / AppKit / GTK4 / Win32 imperatively.
//!
//! HarmonyOS doesn't fit that imperative model: ArkTS owns the UI tree, not
//! native code. So instead of routing perry/ui calls through FFI, this crate
//! walks the HIR pre-codegen, harvests the perry/ui widget tree, and emits
//! it as a real ArkUI `pages/Index.ets` file. The compiled `.so` then has
//! no UI calls at all — Perry's `main()` runs once at NAPI startup for any
//! non-UI logic, and ArkUI declaratively renders the harvested tree.
//!
//! Phase 2 v1.5 scope (visual surface):
//! - `App({body: <expr>})` extraction
//! - `Text(literal)` → `Text('lit').fontSize(20)`
//! - `VStack([...], spacing?)` → `Column({space: <spacing>}) { ... }`
//! - `HStack([...], spacing?)` → `Row({space: <spacing>}) { ... }`
//! - `Button(label, onPress)` → `Button('label')`
//! - `TextField(placeholder, onChange)` → `TextInput({placeholder: 'hint'})`
//! - `Toggle(label, onChange)` → label rendered as Text + ArkUI Toggle in a Row
//! - `Slider(min, max, onChange)` → `Slider({min, max, value: min})`
//! - `Spacer()` → `Blank()`
//! - `Divider()` → `Divider()`
//! - LocalGet escape: `let x = Text("hi"); App({body: x})` follows the
//!   binding back to its init expression for any read-only top-level local.
//!
//! Phase 2 v2 scope (callback bridge):
//! - `Button(label, onPress)` captures `onPress` as a closure, assigns it
//!   a slot id, and emits ArkUI `.onClick(() => perryEntry.invokeCallback(<id>))`.
//!   The closure is then registered into a runtime slot table by an
//!   injected `perry_arkts_register_callback(<id>, <closure>)` call (the
//!   compile harvest pass plants this in `module.init`). On tap, NAPI's
//!   `invokeCallback` looks the slot up and calls the closure via
//!   `js_closure_call0` — running the original Perry TS body.
//! - Toggle/TextField/Slider callbacks are still dropped because their
//!   event payloads (boolean / string / number) need NaN-box marshaling
//!   on the ArkTS → Rust boundary; that's v2.5.
//!
//! State-binding caveat: ArkUI's `@State` / `@Link` reactivity is handled
//! natively in the ArkTS runtime, but Perry's `State<T>` lives in the .so
//! heap and doesn't share memory with the ArkTS heap. Reactive UI updates
//! after a callback (e.g. `count++` re-rendering a `Text(count)`) need a
//! push channel from the .so back to ArkUI; that's a future phase.

use anyhow::Result;
use perry_hir::ir::{Class, Expr, Module, Stmt};
use std::collections::HashMap;

// LocalId is `u32` upstream; re-import directly so we don't carry a
// transitive dep on perry-types just for the type alias.
type LocalId = u32;

/// Result of harvesting an `App({body: ...})` call: the emitted ArkUI
/// source plus the closures that need to be registered into the runtime
/// callback table. Each `callbacks[i]` is the original Perry HIR closure
/// expression at slot `i`; the emitted .ets references it as
/// `perryEntry.invokeCallback(i)`.
pub struct HarvestResult {
    pub ets_source: String,
    pub callbacks: Vec<Expr>,
}

/// Per-id reactive Text registration. `Text("Count: 0", "counter")`
/// registers `id="counter", initial="Count: 0"`. The harvest pass emits
/// `@State text_counter: string = 'Count: 0'` on the page struct and
/// `Text(this.text_counter)` at the widget site; user code calls
/// `setText("counter", newValue)` from inside a closure to rerender.
///
/// Two ids are tracked: `original_id` is the verbatim string the user
/// wrote (used in the switch case, since that's what the runtime drain
/// queue produces), and `field_id` is the ArkTS-safe field-name suffix.
struct TextSlot {
    original_id: String,
    field_id: String,
    initial: String,
}

/// Phase 2 v6 — `state<T>(initial)` registry. Each `let x = state(initial)`
/// declaration in `module.init` registers a synthetic id (`__state_<N>`)
/// + the initial value. Subsequent `x.text()` calls emit reactive Text
/// using the synth id; `x.set(v)` calls inside closures get rewritten to
/// `setText(synth_id, v)` calls (the runtime's `perry_arkts_set_text`
/// already coerces non-string args via `js_jsvalue_to_string`).
struct StateBinding {
    synth_id: String,
    initial_str: String,
}

/// Walk `module.init` for the first `App({...})` call from `perry/ui`,
/// emit the corresponding ArkUI `pages/Index.ets`, capture every
/// closure-bearing arg into `HarvestResult.callbacks` so the compile
/// harvest pass can inject runtime registrations, AND **destructively
/// strip the App call from the HIR** so the LLVM backend doesn't emit
/// `perry_ui_*` FFI calls that would be unresolved on the OHOS target
/// (no `perry-ui-harmonyos` crate exists — UI is rendered declaratively
/// from the emitted `.ets`, not imperatively from native code).
///
/// Returns `Ok(None)` if the module doesn't use `perry/ui App` (the caller
/// should fall through to the blank EntryAbility-only stub; HIR is
/// untouched). Returns `Ok(Some(HarvestResult))` for static-UI programs.
pub fn emit_index_ets(module: &mut Module) -> Result<Option<HarvestResult>> {
    // Snapshot the class table BEFORE the &mut borrow on init so we can
    // look up __AnonShape_* classes (Perry's closed-shape object-literal
    // optimization, v0.5.337+) without aliasing &mut module.
    let classes = module.classes.clone();
    // Phase 2 v6 — pre-walk for `state<T>(initial)` declarations + rewrite
    // `state.set(v)` calls inside the entire module to `setText(synth_id, v)`.
    // This needs to run BEFORE find_and_strip_app + bindings collection so
    // the rewrites land before any harvest detection sees the closures.
    let state_registry = collect_state_bindings(&module.init);
    if !state_registry.is_empty() {
        rewrite_state_calls_in_stmts(&mut module.init, &state_registry);
    }
    // Build a const-binding lookup for top-level `let x = <perry/ui call>;`
    // so the Body can reference a local: `App({body: x})` finds x's init.
    let bindings = collect_const_bindings(&module.init);
    let Some(body_expr) = find_and_strip_app(&mut module.init, &classes) else {
        return Ok(None);
    };
    let mut callbacks: Vec<Expr> = Vec::new();
    let mut text_slots: Vec<TextSlot> = Vec::new();
    let arkts_locals: HashMap<LocalId, String> = HashMap::new();
    let widget_arkui = emit_widget(
        &body_expr,
        &bindings,
        0,
        &mut callbacks,
        &mut text_slots,
        &arkts_locals,
        &classes,
        &state_registry,
    );
    Ok(Some(HarvestResult {
        ets_source: wrap_index_page(&widget_arkui, &text_slots),
        callbacks,
    }))
}

/// Phase 2 v6 — discover top-level `let x = state(initial)` declarations
/// and assign each a synthetic id `__state_<N>`. The initial value is
/// stringified for the v3.2 reactive-Text initial state.
fn collect_state_bindings(init: &[Stmt]) -> HashMap<LocalId, StateBinding> {
    let mut map = HashMap::new();
    let mut counter: usize = 0;
    for stmt in init {
        if let Stmt::Let {
            id,
            init: Some(call_expr),
            ..
        } = stmt
        {
            let initial = match call_expr {
                // Match either `Expr::NativeMethodCall { module: "perry/ui", method: "state", args: [v] }`
                // OR `Expr::Call { callee: Ident("state"), args: [v] }` (whichever
                // shape the perry-hir lowerer produces for the import).
                Expr::NativeMethodCall {
                    module,
                    method,
                    object: None,
                    args,
                    ..
                } if module == "perry/ui" && method == "state" && args.len() == 1 => {
                    Some(args[0].clone())
                }
                _ => None,
            };
            if let Some(initial_expr) = initial {
                let synth_id = format!("__state_{}", counter);
                counter += 1;
                let initial_str = match &initial_expr {
                    Expr::String(s) => s.clone(),
                    Expr::Number(n) => fmt_num(*n),
                    Expr::Integer(n) => format!("{}", n),
                    Expr::Bool(b) => format!("{}", b),
                    _ => "".to_string(),
                };
                map.insert(
                    *id,
                    StateBinding {
                        synth_id,
                        initial_str,
                    },
                );
            }
        }
    }
    map
}

/// Walk a Vec<Stmt> and rewrite any `state.set(v)` calls (where state's
/// LocalId is in the registry) to `setText(synth_id, v)` calls. Recurses
/// into closure bodies, blocks, control flow.
fn rewrite_state_calls_in_stmts(stmts: &mut Vec<Stmt>, reg: &HashMap<LocalId, StateBinding>) {
    for stmt in stmts.iter_mut() {
        rewrite_state_in_stmt(stmt, reg);
    }
}

fn rewrite_state_in_stmt(stmt: &mut Stmt, reg: &HashMap<LocalId, StateBinding>) {
    match stmt {
        Stmt::Expr(e) => rewrite_state_in_expr(e, reg),
        Stmt::Let { init: Some(e), .. } => rewrite_state_in_expr(e, reg),
        Stmt::Return(Some(e)) => rewrite_state_in_expr(e, reg),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_state_in_expr(condition, reg);
            rewrite_state_calls_in_stmts(then_branch, reg);
            if let Some(else_branch) = else_branch {
                rewrite_state_calls_in_stmts(else_branch, reg);
            }
        }
        Stmt::While { condition, body, .. } | Stmt::DoWhile { body, condition, .. } => {
            rewrite_state_in_expr(condition, reg);
            rewrite_state_calls_in_stmts(body, reg);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
            ..
        } => {
            if let Some(init) = init {
                rewrite_state_in_stmt(init.as_mut(), reg);
            }
            if let Some(c) = condition {
                rewrite_state_in_expr(c, reg);
            }
            if let Some(u) = update {
                rewrite_state_in_expr(u, reg);
            }
            rewrite_state_calls_in_stmts(body, reg);
        }
        _ => {}
    }
}

fn rewrite_state_in_expr(e: &mut Expr, reg: &HashMap<LocalId, StateBinding>) {
    // Detect `state.set(v)` first (most specific shape).
    if let Expr::Call { callee, args, .. } = e {
        if args.len() == 1 {
            if let Expr::PropertyGet { object, property } = callee.as_ref() {
                if property == "set" {
                    if let Expr::LocalGet(state_id) = object.as_ref() {
                        if let Some(binding) = reg.get(state_id) {
                            let value_expr = args[0].clone();
                            *e = Expr::NativeMethodCall {
                                module: "perry/ui".to_string(),
                                class_name: None,
                                object: None,
                                method: "setText".to_string(),
                                args: vec![Expr::String(binding.synth_id.clone()), value_expr],
                            };
                            return;
                        }
                    }
                }
            }
        }
    }
    // Recurse into ALL expression children so nested state.set(v) calls
    // inside method args / object literals / closure bodies / etc. are
    // also rewritten. Each variant unrolls its sub-Exprs explicitly so
    // we don't miss any HIR shape.
    match e {
        Expr::Call { callee, args, .. } => {
            rewrite_state_in_expr(callee, reg);
            for a in args.iter_mut() {
                rewrite_state_in_expr(a, reg);
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                rewrite_state_in_expr(o, reg);
            }
            for a in args.iter_mut() {
                rewrite_state_in_expr(a, reg);
            }
        }
        Expr::Object(props) => {
            for (_, v) in props.iter_mut() {
                rewrite_state_in_expr(v, reg);
            }
        }
        Expr::Array(items) => {
            for v in items.iter_mut() {
                rewrite_state_in_expr(v, reg);
            }
        }
        Expr::Closure { body, .. } => {
            rewrite_state_calls_in_stmts(body, reg);
        }
        Expr::PropertyGet { object, .. } => {
            rewrite_state_in_expr(object, reg);
        }
        Expr::PropertySet { object, value, .. } => {
            rewrite_state_in_expr(object, reg);
            rewrite_state_in_expr(value, reg);
        }
        Expr::IndexGet { object, index } => {
            rewrite_state_in_expr(object, reg);
            rewrite_state_in_expr(index, reg);
        }
        Expr::Binary { left, right, .. } => {
            rewrite_state_in_expr(left, reg);
            rewrite_state_in_expr(right, reg);
        }
        Expr::ArrayMap { array, callback } => {
            rewrite_state_in_expr(array, reg);
            rewrite_state_in_expr(callback, reg);
        }
        Expr::New { args, .. } => {
            for a in args.iter_mut() {
                rewrite_state_in_expr(a, reg);
            }
        }
        // Leaf/other variants don't carry rewriteable sub-Exprs (or are
        // rare enough that v6 deferring them is fine — file as v6.5
        // follow-up if anyone hits a real-world miss).
        _ => {}
    }
}

/// Find the first top-level `App({body: <expr>})` call in `module.init`,
/// **return its body by-value**, and replace the entire statement with a
/// no-op `Stmt::Expr(Expr::Number(0.0))`. Other statements are untouched
/// so logic before/after `App(...)` still runs in `perryEntry.run()`.
fn find_and_strip_app(init: &mut [Stmt], classes: &[Class]) -> Option<Expr> {
    for stmt in init.iter_mut() {
        if let Stmt::Expr(Expr::NativeMethodCall {
            module: m,
            method,
            object: None,
            args,
            ..
        }) = stmt
        {
            if m == "perry/ui" && method == "App" && args.len() == 1 {
                let body = extract_body_field(&mut args[0], classes);
                if body.is_some() {
                    *stmt = Stmt::Expr(Expr::Number(0.0));
                    return body;
                }
            }
        }
    }
    None
}

/// Pull out the `body:` field's expression from either a plain
/// `Expr::Object` or a `__AnonShape_*` `Expr::New`. Returns the body by
/// value (cloned for the New case since we can't move out of args[idx]
/// without disturbing the rest of the args array, but the strip below
/// throws the whole call away anyway).
fn extract_body_field(arg: &mut Expr, classes: &[Class]) -> Option<Expr> {
    match arg {
        Expr::Object(props) => {
            let idx = props.iter().position(|(k, _)| k == "body")?;
            let (_, body) = props.remove(idx);
            Some(body)
        }
        Expr::New {
            class_name, args, ..
        } if class_name.starts_with("__AnonShape_") => {
            let class = classes.iter().find(|c| &c.name == class_name)?;
            let body_idx = class.fields.iter().position(|f| f.name == "body")?;
            args.get(body_idx).cloned()
        }
        _ => None,
    }
}

/// Snapshot read-only top-level `let x = <expr>;` so widget walks can
/// follow `Expr::LocalGet(x)` back to the init expression. We index by
/// LocalId rather than name because perry-hir's identifier resolution
/// runs by id — names are debug aids only.
///
/// Phase 2 v1.5 only follows TOP-level inits; nested let-bindings inside
/// blocks would need a wider analysis pass (the code path is only invoked
/// via `App({body: x})` which itself is top-level, so the binding it
/// references is also top-level — works for the common case).
fn collect_const_bindings(init: &[Stmt]) -> HashMap<LocalId, Expr> {
    let mut map = HashMap::new();
    for stmt in init {
        if let Stmt::Let {
            id,
            init: Some(expr),
            mutable: false,
            ..
        } = stmt
        {
            map.insert(*id, expr.clone());
        }
    }
    map
}

/// Resolve `Expr::LocalGet(id)` to its bound init expression if available.
/// Returns the original expression for any non-LocalGet shape so callers
/// can use it as a transparent identity-or-deref helper.
fn resolve(expr: &Expr, bindings: &HashMap<LocalId, Expr>) -> Expr {
    if let Expr::LocalGet(id) = expr {
        if let Some(init) = bindings.get(id) {
            return init.clone();
        }
    }
    expr.clone()
}

/// Emit an ArkUI expression for a perry/ui widget call. Returns the inner
/// `build()`-block content (no wrapping component). `depth` controls
/// indentation when emitting nested children. `callbacks` accumulates
/// closure expressions that need runtime registration; each push assigns
/// the next slot id (= callbacks.len() before push).
///
/// Unrecognized widgets degrade to a comment + a placeholder Text — never
/// errors out, since emit-time errors would leave the user without any UI.
#[allow(clippy::too_many_arguments)]
fn emit_widget(
    expr: &Expr,
    bindings: &HashMap<LocalId, Expr>,
    depth: usize,
    callbacks: &mut Vec<Expr>,
    text_slots: &mut Vec<TextSlot>,
    arkts_locals: &HashMap<LocalId, String>,
    classes: &[Class],
    state_registry: &HashMap<LocalId, StateBinding>,
) -> String {
    // Phase 2 v6 — `state.text()` shape: Expr::Call { callee: PropertyGet
    // { obj: LocalGet(state_id), property: "text" }, args: [] } where
    // state_id is in the registry. Emit a reactive Text using the
    // registered synth_id + initial value (uses the v3.2 path).
    if let Expr::Call { callee, args, .. } = expr {
        if args.is_empty() {
            if let Expr::PropertyGet { object, property } = callee.as_ref() {
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
                "Text" => emit_text(args, text_slots, arkts_locals),
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
                ),
                "Button" => emit_button(args, callbacks),
                "TextField" => emit_textfield(args, callbacks),
                "Toggle" => emit_toggle(args, callbacks),
                "Slider" => emit_slider(args, callbacks),
                "Spacer" => "Blank()".to_string(),
                "Divider" => "Divider()".to_string(),
                "Image" | "ImageFile" => emit_image(args),
                "ScrollView" => emit_scrollview(
                    args,
                    bindings,
                    depth,
                    callbacks,
                    text_slots,
                    arkts_locals,
                    classes,
                state_registry,
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
                ),
                "Picker" => emit_picker(args, callbacks),
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
                ),
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
            if let Some(props) = style_props {
                let modifiers = emit_style_modifiers(&props);
                if !modifiers.is_empty() {
                    return format!("{}{}", core, modifiers);
                }
            }
            core
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
        ),
        _ => format!(
            "// unrecognized body expression (must be a perry/ui widget call)\n\
             Text('[unrecognized body]').fontSize(16).fontColor('#888888')"
        ),
    }
}

/// Phase 2 v5: emit ArkUI `ForEach(<array>, (__item) => { <body> })`
/// from a `Expr::ArrayMap { array, callback }` HIR node. The callback's
/// closure parameter is bound to `__item` in arkts_locals so any
/// `LocalGet(param_id)` inside the body resolves correctly.
///
/// The array source must be a literal `Expr::Array` or a `LocalGet`
/// that resolves to a top-level binding (via `bindings`). Other shapes
/// (e.g., complex computed expressions) fall back to a degraded inline
/// emit so the build doesn't break.
#[allow(clippy::too_many_arguments)]
fn emit_for_each(
    array: &Expr,
    callback: &Expr,
    bindings: &HashMap<LocalId, Expr>,
    depth: usize,
    callbacks: &mut Vec<Expr>,
    text_slots: &mut Vec<TextSlot>,
    arkts_locals: &HashMap<LocalId, String>,
    classes: &[Class],
    state_registry: &HashMap<LocalId, StateBinding>,
) -> String {
    let array_src = arkts_array_source(array, bindings);
    let (param_id, body_expr) = match callback {
        Expr::Closure { params, body, .. } if !params.is_empty() => {
            // The closure body is a Vec<Stmt>; we expect a single return-
            // expr or expression-statement. Take the first Expr we find.
            let body_expr = body.iter().find_map(|s| match s {
                Stmt::Return(Some(e)) => Some(e.clone()),
                Stmt::Expr(e) => Some(e.clone()),
                _ => None,
            });
            (Some(params[0].id), body_expr)
        }
        _ => (None, None),
    };
    let inner_indent = "    ".repeat(depth + 1);
    let outer_indent = "    ".repeat(depth);
    let (param_name, body_str) = match (param_id, body_expr) {
        (Some(pid), Some(body)) => {
            let mut locals = arkts_locals.clone();
            locals.insert(pid, "__item".to_string());
            let inner = emit_widget(
                &body,
                bindings,
                depth + 1,
                callbacks,
                text_slots,
                &locals,
                classes,
                state_registry,
            );
            ("__item".to_string(), inner)
        }
        _ => (
            "__item".to_string(),
            "Text('[non-closure ForEach body]').fontSize(16).fontColor('#888888')".to_string(),
        ),
    };
    let indented_body = body_str
        .lines()
        .map(|l| format!("{}{}", inner_indent, l))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "ForEach({arr}, ({pname}: any) => {{\n\
         {body}\n\
         {outer}}}, ({pname}: any) => {pname})",
        arr = array_src,
        pname = param_name,
        body = indented_body,
        outer = outer_indent,
    )
}

/// Emit a TS expression for the array source of a ForEach. Supports
/// literal `Expr::Array(items)` (serialized inline) and `Expr::LocalGet`
/// resolved to a top-level binding's name. Other shapes fall back to
/// an empty `[]` with a note comment.
fn arkts_array_source(e: &Expr, bindings: &HashMap<LocalId, Expr>) -> String {
    match e {
        Expr::Array(items) => {
            let parts: Vec<String> = items.iter().map(arkts_value_literal).collect();
            format!("[{}]", parts.join(", "))
        }
        Expr::LocalGet(_id) => {
            // Look up the binding's init expr; if it's an Array literal,
            // serialize. Otherwise fall through to empty.
            if let Expr::LocalGet(id) = e {
                if let Some(Expr::Array(items)) = bindings.get(id) {
                    let parts: Vec<String> = items.iter().map(arkts_value_literal).collect();
                    return format!("[{}]", parts.join(", "));
                }
            }
            // Phase 2 v5 limitation: complex array sources need real
            // ArkTS-side state binding. Emit a placeholder.
            "[/* unresolved ForEach source — needs Phase 2 v6 state binding */]".to_string()
        }
        _ => "[/* unsupported ForEach source */]".to_string(),
    }
}

/// Serialize a literal-shaped `Expr` to TS source for inline array lit.
fn arkts_value_literal(e: &Expr) -> String {
    match e {
        Expr::String(s) => arkts_string_lit(s),
        Expr::Number(n) => fmt_num(*n),
        Expr::Integer(n) => format!("{}", n),
        Expr::Bool(b) => format!("{}", b),
        _ => "null".to_string(),
    }
}

/// `Text("hi")` → `Text('hi').fontSize(20)`.
///
/// Phase 2 v3 Option 2: `Text("hi", "id")` → registers a reactive slot.
/// The widget emits `Text(this.text_<id>)` instead of a string literal,
/// and `wrap_index_page` adds `@State text_<id>: string = 'hi'` to the
/// page struct. User code calls `setText("id", newValue)` from inside
/// a closure to update.
///
/// Non-string-literal args fall back to a placeholder so unsupported
/// shapes don't break the build.
fn emit_text(
    args: &[Expr],
    text_slots: &mut Vec<TextSlot>,
    arkts_locals: &HashMap<LocalId, String>,
) -> String {
    // Phase 2 v5: inside a ForEach body, `Text(item)` where `item` is
    // the closure's loop param resolves via arkts_locals → `Text(__item)`.
    let first = args.first();
    let content_str = match first {
        Some(Expr::String(content)) => Some(arkts_string_lit(content)),
        Some(Expr::LocalGet(id)) => arkts_locals.get(id).cloned(),
        _ => None,
    };
    let Some(content_arg) = content_str else {
        return "Text('[non-literal Text arg]').fontSize(20).fontColor('#888888')".to_string();
    };
    if let Some(Expr::String(id)) = args.get(1) {
        // Reactive Text. Sanitize the id so it's a valid ArkTS field-
        // name suffix (alphanumeric + underscore). The original id stays
        // alongside it for the runtime-side switch match.
        // Only the literal-string form is reactive — ForEach's __item
        // binding is per-iteration and doesn't persist to a slot.
        if let Some(Expr::String(initial)) = first {
            let safe = sanitize_text_id(id);
            text_slots.push(TextSlot {
                original_id: id.clone(),
                field_id: safe.clone(),
                initial: initial.clone(),
            });
            return format!("Text(this.text_{}).fontSize(20)", safe);
        }
    }
    format!("Text({}).fontSize(20)", content_arg)
}

/// Extract a `style: {...}` object from a widget arg. Handles both
/// `Expr::Object(props)` (open shape) and Perry's closed-shape
/// optimization `Expr::New { class_name: "__AnonShape_*", args }` where
/// the class's fields list correlates positionally with args. Used by
/// `emit_style_modifiers` to map StyleProps into ArkUI modifiers.
///
/// Phase 2 v5 — ergonomic parity with macOS/iOS/etc inline styling.
fn extract_style_object(arg: &Expr, classes: &[Class]) -> Option<Vec<(String, Expr)>> {
    match arg {
        Expr::Object(props) => Some(props.clone()),
        Expr::New {
            class_name, args, ..
        } if class_name.starts_with("__AnonShape_") => {
            let class = classes.iter().find(|c| &c.name == class_name)?;
            // Pair each field with its positional arg; missing args fall through.
            let pairs: Vec<(String, Expr)> = class
                .fields
                .iter()
                .enumerate()
                .filter_map(|(i, f)| args.get(i).map(|a| (f.name.clone(), a.clone())))
                .collect();
            Some(pairs)
        }
        _ => None,
    }
}

/// Map a Perry color expression to an ArkUI color string.
///   - `Expr::String("blue")` / `"#3B82F6"` → quoted string passthrough
///   - `Expr::Object([(r,…),(g,…),(b,…),(a,…)])` (PerryColor) → `'rgba(R,G,B,A)'`
///     where channels are scaled to 0..255 / 0..1 per CSS rgba() convention
fn arkts_color_value(e: &Expr) -> String {
    match e {
        Expr::String(s) => arkts_string_lit(s),
        Expr::Object(props) => {
            let chan = |name: &str, default: f64| -> f64 {
                props
                    .iter()
                    .find(|(k, _)| k == name)
                    .and_then(|(_, v)| match v {
                        Expr::Number(n) => Some(*n),
                        Expr::Integer(n) => Some(*n as f64),
                        _ => None,
                    })
                    .unwrap_or(default)
            };
            let r = (chan("r", 0.0) * 255.0).round() as i64;
            let g = (chan("g", 0.0) * 255.0).round() as i64;
            let b = (chan("b", 0.0) * 255.0).round() as i64;
            let a = chan("a", 1.0);
            format!("'rgba({}, {}, {}, {})'", r, g, b, fmt_num(a))
        }
        _ => "'#000000'".to_string(),
    }
}

/// Map a `StyleProps` object to an ArkUI modifier chain like
/// `.backgroundColor('blue').borderRadius(8).opacity(0.95)`.
///
/// Phase 2 v5 covers the high-traffic props: backgroundColor, color,
/// fontSize, fontWeight, fontFamily, borderRadius, padding, opacity,
/// hidden, borderColor + borderWidth (as combined `.border({...})`).
/// Skipped (complex / multi-arg ArkUI shape): shadow, gradient,
/// textDecoration, tooltip, animation, transition — these would each
/// need their own ArkUI modifier and are deferred to Phase 2 v13.
fn emit_style_modifiers(props: &[(String, Expr)]) -> String {
    let mut out = String::new();
    let mut border_color: Option<String> = None;
    let mut border_width: Option<String> = None;
    for (k, v) in props {
        match k.as_str() {
            "backgroundColor" => {
                out.push_str(&format!(".backgroundColor({})", arkts_color_value(v)));
            }
            "color" => {
                // ArkUI's `.fontColor` works on Text; non-text widgets
                // silently ignore it.
                out.push_str(&format!(".fontColor({})", arkts_color_value(v)));
            }
            "fontSize" => {
                if let Some(n) = numeric_expr(v) {
                    out.push_str(&format!(".fontSize({})", fmt_num(n)));
                }
            }
            "fontWeight" => {
                if let Some(n) = numeric_expr(v) {
                    out.push_str(&format!(".fontWeight({})", fmt_num(n)));
                }
            }
            "fontFamily" => {
                if let Expr::String(s) = v {
                    out.push_str(&format!(".fontFamily({})", arkts_string_lit(s)));
                }
            }
            "borderRadius" => {
                if let Some(n) = numeric_expr(v) {
                    out.push_str(&format!(".borderRadius({})", fmt_num(n)));
                }
            }
            "borderColor" => {
                border_color = Some(arkts_color_value(v));
            }
            "borderWidth" => {
                if let Some(n) = numeric_expr(v) {
                    border_width = Some(fmt_num(n));
                }
            }
            "padding" => match v {
                Expr::Number(n) => out.push_str(&format!(".padding({})", fmt_num(*n))),
                Expr::Integer(n) => out.push_str(&format!(".padding({})", *n)),
                Expr::Object(sides) => {
                    let side = |name: &str| -> Option<f64> {
                        sides
                            .iter()
                            .find(|(k, _)| k == name)
                            .and_then(|(_, v)| numeric_expr(v))
                    };
                    let parts: Vec<String> = ["top", "right", "bottom", "left"]
                        .iter()
                        .filter_map(|s| side(s).map(|n| format!("{}: {}", s, fmt_num(n))))
                        .collect();
                    if !parts.is_empty() {
                        out.push_str(&format!(".padding({{ {} }})", parts.join(", ")));
                    }
                }
                _ => {}
            },
            "opacity" => {
                if let Some(n) = numeric_expr(v) {
                    out.push_str(&format!(".opacity({})", fmt_num(n)));
                }
            }
            "hidden" => {
                let is_hidden = matches!(v, Expr::Bool(true));
                if is_hidden {
                    out.push_str(".visibility(Visibility.Hidden)");
                }
            }
            // Phase 2 v5 deferred: shadow, gradient, textDecoration,
            // tooltip, animation, transition — see Phase 2 v13.
            _ => {}
        }
    }
    // Joint border: ArkUI's `.border({color, width})` is one modifier
    // taking a config object; emit only if at least one was set.
    if border_color.is_some() || border_width.is_some() {
        let mut parts: Vec<String> = Vec::new();
        if let Some(w) = border_width {
            parts.push(format!("width: {}", w));
        }
        if let Some(c) = border_color {
            parts.push(format!("color: {}", c));
        }
        out.push_str(&format!(".border({{ {} }})", parts.join(", ")));
    }
    out
}

/// Extract a Number / Integer expression as `f64`. Returns None for
/// anything else (including `Expr::String` parseable numerals — those
/// are intentionally rejected because StyleProps forbids them).
fn numeric_expr(e: &Expr) -> Option<f64> {
    match e {
        Expr::Number(n) => Some(*n),
        Expr::Integer(n) => Some(*n as f64),
        _ => None,
    }
}

/// Sanitize an arbitrary string id into a valid ArkTS field-name suffix.
/// Replaces non-[a-zA-Z0-9_] with `_`. Front-pads with `x` if it starts
/// with a digit. Empty input → `default`.
fn sanitize_text_id(s: &str) -> String {
    if s.is_empty() {
        return "default".to_string();
    }
    let mut out: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out
        .chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
    {
        out.insert(0, 'x');
    }
    out
}

/// VStack/HStack: detect (Array, ...) vs (Number, Array, ...) signatures.
/// Recurse into the children array via `emit_widget`. Spacing prop
/// becomes `Column({space: <n>})` / `Row({space: <n>})`. ArkUI's default
/// of 0 makes spacing-less stacks look cramped, so we default to 8 which
/// matches the perry-ui-macos default.
#[allow(clippy::too_many_arguments)]
fn emit_stack(
    arkui_kind: &str,
    args: &[Expr],
    bindings: &HashMap<LocalId, Expr>,
    depth: usize,
    callbacks: &mut Vec<Expr>,
    text_slots: &mut Vec<TextSlot>,
    arkts_locals: &HashMap<LocalId, String>,
    classes: &[Class],
    state_registry: &HashMap<LocalId, StateBinding>,
) -> String {
    // First-arg shape detection — same logic as lower_call/native.rs:91.
    let (spacing, children_idx) = match args.first() {
        Some(Expr::Array(_)) | Some(Expr::ArrayMap { .. }) => (8.0, 0),
        Some(Expr::Number(n)) => (*n, 1),
        Some(Expr::Integer(n)) => (*n as f64, 1),
        _ => (8.0, 0),
    };

    let children = match args.get(children_idx) {
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
        )],
        Some(_) => vec![format!(
            "// children arg wasn't an array literal — Phase 2 v1.5 limitation\n\
             Text('[non-array children]').fontSize(16).fontColor('#888888')"
        )],
        None => vec![],
    };

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

/// `Button("label", onPress)` → `Button('label').onClick(() => { ... })`.
/// The onClick body invokes the registered closure via NAPI then drains
/// the toast queue (Phase 2 v3 Option 1):
///
/// ```text
/// perryEntry.invokeCallback(<idx>);
/// let __t = perryEntry.drainToast();
/// while (__t !== undefined) {
///     promptAction.showToast({ message: __t });
///     __t = perryEntry.drainToast();
/// }
/// ```
///
/// The drain loop runs unconditionally — most closures don't enqueue
/// toasts, so it's a single fast `drainToast()` returning undefined.
/// When the user calls `showToast("Saved!")` from inside the closure,
/// the message lands on the queue and pops out here as a popup banner.
///
/// Non-closure second args (or absent) emit a label-only Button with no
/// onClick — preserves v1.5 behavior for simpler tests.
fn emit_button(args: &[Expr], callbacks: &mut Vec<Expr>) -> String {
    let label = first_string_arg(args).unwrap_or_else(|| "Button".to_string());
    let onclick_attached = match args.get(1) {
        Some(closure @ Expr::Closure { .. }) => {
            let idx = callbacks.len();
            callbacks.push(closure.clone());
            format!(
                ".onClick(() => {{\n    \
                 perryEntry.invokeCallback({});\n    \
                 {drain}\
                 }})",
                idx,
                drain = drain_loop_body()
            )
        }
        _ => String::new(),
    };
    format!(
        "Button({}).fontSize(16){}",
        arkts_string_lit(&label),
        onclick_attached
    )
}

/// Three-pass drain after a closure body returns. Used by Button.onClick
/// (Phase 2 v2) and Toggle/TextField/Slider.onChange (v2.5):
///   1. drainToast loop → promptAction.showToast({message})
///   2. drainTextUpdate loop → this.applyTextUpdate(id, value)
/// `invokeCallback` itself is emitted by the caller because it varies
/// (callN with N-arg widgets, plus ArkUI's per-widget onChange shape).
fn drain_loop_body() -> String {
    "let __t = perryEntry.drainToast();\n    \
     while (__t !== undefined) { \
     promptAction.showToast({ message: __t }); \
     __t = perryEntry.drainToast(); \
     }\n    \
     let __u = perryEntry.drainTextUpdate();\n    \
     while (__u !== undefined) { \
     this.applyTextUpdate(__u.id, __u.value); \
     __u = perryEntry.drainTextUpdate(); \
     }\n  "
        .to_string()
}

/// `TextField(placeholder, onChange)` → `TextInput(...).onChange(...)`.
/// Phase 2 v2.5: when `onChange` is a closure, register it in the slot
/// table and emit an `onChange((value: string) => perryEntry.invokeCallback1(idx, value))`
/// handler that also drains toast + text-update queues.
fn emit_textfield(args: &[Expr], callbacks: &mut Vec<Expr>) -> String {
    let placeholder = first_string_arg(args).unwrap_or_default();
    let onchange = match args.get(1) {
        Some(closure @ Expr::Closure { .. }) => {
            let idx = callbacks.len();
            callbacks.push(closure.clone());
            format!(
                ".onChange((value: string) => {{\n    \
                 perryEntry.invokeCallback1({}, value);\n    \
                 {drain}\
                 }})",
                idx,
                drain = drain_loop_body()
            )
        }
        _ => String::new(),
    };
    format!(
        "TextInput({{ placeholder: {} }}){}",
        arkts_string_lit(&placeholder),
        onchange,
    )
}

/// `Toggle(label, onChange)` → label as a sibling Text + ArkUI's Toggle
/// in a Row. Phase 2 v2.5: closure receives `(isOn: boolean)`.
fn emit_toggle(args: &[Expr], callbacks: &mut Vec<Expr>) -> String {
    let label = first_string_arg(args).unwrap_or_default();
    let onchange = match args.get(1) {
        Some(closure @ Expr::Closure { .. }) => {
            let idx = callbacks.len();
            callbacks.push(closure.clone());
            format!(
                ".onChange((isOn: boolean) => {{\n    \
                 perryEntry.invokeCallback1({}, isOn);\n    \
                 {drain}\
                 }})",
                idx,
                drain = drain_loop_body()
            )
        }
        _ => String::new(),
    };
    if label.is_empty() {
        format!(
            "Toggle({{ type: ToggleType.Switch, isOn: false }}){}",
            onchange
        )
    } else {
        format!(
            "Row({{ space: 8 }}) {{\n\
             \x20\x20\x20\x20Text({}).fontSize(16)\n\
             \x20\x20\x20\x20Toggle({{ type: ToggleType.Switch, isOn: false }}){}\n\
             }}",
            arkts_string_lit(&label),
            onchange,
        )
    }
}

/// `Slider(min, max, onChange)` → ArkUI Slider with onChange. Phase 2
/// v2.5: closure receives `(value: number)`. ArkUI's onChange callback
/// is `(value: number, mode: SliderChangeMode)` — we ignore `mode` and
/// only forward `value`.
fn emit_slider(args: &[Expr], callbacks: &mut Vec<Expr>) -> String {
    let min = numeric_arg(args, 0).unwrap_or(0.0);
    let max = numeric_arg(args, 1).unwrap_or(100.0);
    let onchange = match args.get(2) {
        Some(closure @ Expr::Closure { .. }) => {
            let idx = callbacks.len();
            callbacks.push(closure.clone());
            format!(
                ".onChange((value: number, _mode: SliderChangeMode) => {{\n    \
                 perryEntry.invokeCallback1({}, value);\n    \
                 {drain}\
                 }})",
                idx,
                drain = drain_loop_body()
            )
        }
        _ => String::new(),
    };
    format!(
        "Slider({{ value: {min}, min: {min}, max: {max}, step: 1, style: SliderStyle.OutSet }}){onchange}",
        min = fmt_num(min),
        max = fmt_num(max),
        onchange = onchange,
    )
}

/// `Image(src)` / `ImageFile(src)` → `Image('src').width('100%').height(200)`.
/// Default sizing matches the perry-ui-* native default of "fill width,
/// 200pt tall"; users can wrap in further sizing via container modifiers
/// later (Phase 2 v5 will likely accept a `style: { ... }` trailing arg).
/// Non-string-literal args fall back to a placeholder Text so unsupported
/// shapes don't break the build.
fn emit_image(args: &[Expr]) -> String {
    let Some(Expr::String(src)) = args.first() else {
        return "Text('[non-literal Image src]').fontSize(16).fontColor('#888888')".to_string();
    };
    format!("Image({}).width('100%').height(200)", arkts_string_lit(src))
}

/// `ScrollView(children)` → `Scroll() { Column({space: 8}) { ... } }`.
/// ArkUI's `Scroll` is a single-child container that scrolls vertically by
/// default; we wrap in a `Column` so multiple children stack the way users
/// expect from the perry-ui-* native ScrollView wiring. Empty / non-array
/// children degrade to an empty Scroll just like the native variant.
#[allow(clippy::too_many_arguments)]
fn emit_scrollview(
    args: &[Expr],
    bindings: &HashMap<LocalId, Expr>,
    depth: usize,
    callbacks: &mut Vec<Expr>,
    text_slots: &mut Vec<TextSlot>,
    arkts_locals: &HashMap<LocalId, String>,
    classes: &[Class],
    state_registry: &HashMap<LocalId, StateBinding>,
) -> String {
    let inner_indent = "    ".repeat(depth + 2);
    let mid_indent = "    ".repeat(depth + 1);
    let outer_indent = "    ".repeat(depth);

    let children: Vec<String> = match args.first() {
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
        )],
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
fn emit_lazy_vstack(
    args: &[Expr],
    bindings: &HashMap<LocalId, Expr>,
    depth: usize,
    callbacks: &mut Vec<Expr>,
    text_slots: &mut Vec<TextSlot>,
    arkts_locals: &HashMap<LocalId, String>,
    classes: &[Class],
    state_registry: &HashMap<LocalId, StateBinding>,
) -> String {
    let inner_indent = "    ".repeat(depth + 1);
    let outer_indent = "    ".repeat(depth);

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
        )],
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
        "// LazyVStack: rendered eagerly as Column. Real lazy support needs\n\
         {outer}// LazyForEach + IDataSource (Phase 2 v10 follow-up).\n\
         {outer}Column({{ space: 8 }}) {{\n\
         {body}\n\
         {outer}}}",
        outer = outer_indent,
        body = body,
    )
}

/// `Picker(options, onChange)` → ArkUI `TextPicker({range, value: range[0]}).onChange(...)`.
/// Closure receives `(idx: number)` matching the perry-ui-* TS surface.
/// ArkUI's onChange has the shape `(value: string, index: number)` — we
/// forward only `index` since that's what the Perry callback expects.
/// Same drain pattern as Toggle/Slider.
fn emit_picker(args: &[Expr], callbacks: &mut Vec<Expr>) -> String {
    let options = match args.first() {
        Some(Expr::Array(items)) => {
            let strs: Vec<String> = items
                .iter()
                .filter_map(|item| match item {
                    Expr::String(s) => Some(arkts_string_lit(s)),
                    _ => None,
                })
                .collect();
            format!("[{}]", strs.join(", "))
        }
        _ => "[]".to_string(),
    };
    // ArkUI requires a `value` field set to a member of `range`; falling
    // back to an empty string is safe when options is empty.
    let initial = match args.first() {
        Some(Expr::Array(items)) => match items.first() {
            Some(Expr::String(s)) => arkts_string_lit(s),
            _ => "''".to_string(),
        },
        _ => "''".to_string(),
    };

    let onchange = match args.get(1) {
        Some(closure @ Expr::Closure { .. }) => {
            let idx = callbacks.len();
            callbacks.push(closure.clone());
            format!(
                ".onChange((_value: string, index: number) => {{\n    \
                 perryEntry.invokeCallback1({}, index);\n    \
                 {drain}\
                 }})",
                idx,
                drain = drain_loop_body()
            )
        }
        _ => String::new(),
    };

    format!(
        "TextPicker({{ range: {opts}, value: {init} }}){onchange}",
        opts = options,
        init = initial,
        onchange = onchange,
    )
}

/// `ProgressView(value?, total?)` → ArkUI `Progress({value, total, type: ProgressType.Linear})`.
/// Defaults: value=0, total=100. Both args optional — leaf widget, no
/// callbacks, no children.
fn emit_progressview(args: &[Expr]) -> String {
    let value = numeric_arg(args, 0).unwrap_or(0.0);
    let total = numeric_arg(args, 1).unwrap_or(100.0);
    format!(
        "Progress({{ value: {value}, total: {total}, type: ProgressType.Linear }})",
        value = fmt_num(value),
        total = fmt_num(total),
    )
}

/// `Section(title, children)` → labeled vertical group.
/// Emits `Column({space: 4}) { Text('<title>').fontSize(14).fontColor('#888888'); <children> }`.
/// The greyed-out small label header matches the iOS UITableView section
/// header convention; no native ArkUI primitive maps 1:1, so we hand-roll.
#[allow(clippy::too_many_arguments)]
fn emit_section(
    args: &[Expr],
    bindings: &HashMap<LocalId, Expr>,
    depth: usize,
    callbacks: &mut Vec<Expr>,
    text_slots: &mut Vec<TextSlot>,
    arkts_locals: &HashMap<LocalId, String>,
    classes: &[Class],
    state_registry: &HashMap<LocalId, StateBinding>,
) -> String {
    let title = first_string_arg(args).unwrap_or_default();

    let inner_indent = "    ".repeat(depth + 1);
    let outer_indent = "    ".repeat(depth);

    let children: Vec<String> = match args.get(1) {
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
        )],
        _ => vec![],
    };

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
fn emit_tabs(
    args: &[Expr],
    bindings: &HashMap<LocalId, Expr>,
    depth: usize,
    callbacks: &mut Vec<Expr>,
    text_slots: &mut Vec<TextSlot>,
    arkts_locals: &HashMap<LocalId, String>,
    classes: &[Class],
    state_registry: &HashMap<LocalId, StateBinding>,
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
fn emit_modal(_args: &[Expr], _callbacks: &mut Vec<Expr>) -> String {
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
fn emit_menu(args: &[Expr], callbacks: &mut Vec<Expr>) -> String {
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
fn emit_grid(
    args: &[Expr],
    bindings: &HashMap<LocalId, Expr>,
    depth: usize,
    callbacks: &mut Vec<Expr>,
    text_slots: &mut Vec<TextSlot>,
    arkts_locals: &HashMap<LocalId, String>,
    classes: &[Class],
    state_registry: &HashMap<LocalId, StateBinding>,
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
fn wrap_index_page(widget_body: &str, text_slots: &[TextSlot]) -> String {
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

    format!(
        "// Auto-generated by Perry (perry-codegen-arkts) — do not edit.\n\
         // Regenerated every `perry compile --target harmonyos`.\n\
         //\n\
         // Source of truth is the `App({{body: ...}})` call in your\n\
         // TypeScript entry. Edit there; this file is overwritten.\n\
         import perryEntry from 'libentry.so';\n\
         import promptAction from '@ohos.promptAction';\n\
         \n\
         @Entry\n\
         @Component\n\
         struct Index {{\n\
         {states}\
         {apply}\
         \x20\x20\x20\x20build() {{\n\
         \x20\x20\x20\x20\x20\x20\x20\x20Column() {{\n\
         {body}\n\
         \x20\x20\x20\x20\x20\x20\x20\x20}}\n\
         \x20\x20\x20\x20\x20\x20\x20\x20.width('100%')\n\
         \x20\x20\x20\x20\x20\x20\x20\x20.height('100%')\n\
         \x20\x20\x20\x20\x20\x20\x20\x20.justifyContent(FlexAlign.Center)\n\
         \x20\x20\x20\x20}}\n\
         }}\n",
        states = state_decls,
        apply = apply_method,
        body = indented
    )
}

// ----- helpers -----

/// First arg matched as a string literal. Returns None if absent or
/// non-literal so callers can pick a sensible default.
fn first_string_arg(args: &[Expr]) -> Option<String> {
    match args.first() {
        Some(Expr::String(s)) => Some(s.clone()),
        _ => None,
    }
}

/// Get arg at `idx` as a Number, supporting both Integer and Number HIR
/// variants since perry-hir distinguishes them.
fn numeric_arg(args: &[Expr], idx: usize) -> Option<f64> {
    match args.get(idx) {
        Some(Expr::Number(n)) => Some(*n),
        Some(Expr::Integer(n)) => Some(*n as f64),
        _ => None,
    }
}

/// Format a float as ArkTS source. Whole numbers emit without a decimal
/// (`8`, not `8.0`) to match ArkUI's idiomatic style.
fn fmt_num(n: f64) -> String {
    if n == n.trunc() && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

/// Escape a Rust string into an ArkTS single-quoted string literal.
/// ArkTS shares JS string-literal rules — escape backslash + single quote.
fn arkts_string_lit(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
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
            init: vec![],
            exported_native_instances: vec![],
            exported_func_return_native_instances: vec![],
            exported_objects: vec![],
            exported_functions: vec![],
            widgets: vec![],
            uses_fetch: false,
            extern_funcs: vec![],
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
            enclosing_class: None,
            is_async: false,
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
            is_rest: false,
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
                enclosing_class: None,
                is_async: false,
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
            enclosing_class: None,
            is_async: false,
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
            assert!(has_settext, "closure body should have been rewritten to setText");
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
        assert!(r.ets_source.contains("@State text___state_1: string = 'Alice'"));
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
        // The deferral note is part of the source so a future contributor
        // sees the LazyForEach + IDataSource follow-up at the call site.
        assert!(r
            .ets_source
            .contains("LazyVStack: rendered eagerly as Column"));
        assert!(r.ets_source.contains("Column({ space: 8 })"));
        assert!(r.ets_source.contains("Text('row 0')"));
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
}
