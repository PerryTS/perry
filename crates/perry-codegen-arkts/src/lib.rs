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
    // Build a const-binding lookup for top-level `let x = <perry/ui call>;`
    // so the Body can reference a local: `App({body: x})` finds x's init.
    // Cloning the Stmt list is cheap relative to codegen; avoids a second
    // mutable-borrow pass over init.
    let bindings = collect_const_bindings(&module.init);
    let Some(body_expr) = find_and_strip_app(&mut module.init, &classes) else {
        return Ok(None);
    };
    let mut callbacks: Vec<Expr> = Vec::new();
    let mut text_slots: Vec<TextSlot> = Vec::new();
    let widget_arkui = emit_widget(&body_expr, &bindings, 0, &mut callbacks, &mut text_slots);
    Ok(Some(HarvestResult {
        ets_source: wrap_index_page(&widget_arkui, &text_slots),
        callbacks,
    }))
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
fn emit_widget(
    expr: &Expr,
    bindings: &HashMap<LocalId, Expr>,
    depth: usize,
    callbacks: &mut Vec<Expr>,
    text_slots: &mut Vec<TextSlot>,
) -> String {
    let resolved = resolve(expr, bindings);
    match &resolved {
        Expr::NativeMethodCall {
            module: m,
            method,
            args,
            ..
        } if m == "perry/ui" => match method.as_str() {
            "Text" => emit_text(args, text_slots),
            "VStack" => emit_stack("Column", args, bindings, depth, callbacks, text_slots),
            "HStack" => emit_stack("Row", args, bindings, depth, callbacks, text_slots),
            "Button" => emit_button(args, callbacks),
            "TextField" => emit_textfield(args, callbacks),
            "Toggle" => emit_toggle(args, callbacks),
            "Slider" => emit_slider(args, callbacks),
            "Spacer" => "Blank()".to_string(),
            "Divider" => "Divider()".to_string(),
            // Phase 2 v4 widgets.
            "Image" | "ImageFile" => emit_image(args),
            "ScrollView" => emit_scrollview(args, bindings, depth, callbacks, text_slots),
            "LazyVStack" => emit_lazy_vstack(args, bindings, depth, callbacks, text_slots),
            "Picker" => emit_picker(args, callbacks),
            "ProgressView" => emit_progressview(args),
            "Section" => emit_section(args, bindings, depth, callbacks, text_slots),
            other => format!(
                "// unsupported perry/ui widget: {} (Phase 2 v4)\n\
                 Text('[unsupported: {}]').fontSize(16).fontColor('#888888')",
                other, other
            ),
        },
        _ => format!(
            "// unrecognized body expression (must be a perry/ui widget call)\n\
             Text('[unrecognized body]').fontSize(16).fontColor('#888888')"
        ),
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
fn emit_text(args: &[Expr], text_slots: &mut Vec<TextSlot>) -> String {
    let Some(Expr::String(content)) = args.first() else {
        return "Text('[non-literal Text arg]').fontSize(20).fontColor('#888888')".to_string();
    };
    if let Some(Expr::String(id)) = args.get(1) {
        // Reactive Text. Sanitize the id so it's a valid ArkTS field-
        // name suffix (alphanumeric + underscore). The original id stays
        // alongside it for the runtime-side switch match.
        let safe = sanitize_text_id(id);
        text_slots.push(TextSlot {
            original_id: id.clone(),
            field_id: safe.clone(),
            initial: content.clone(),
        });
        format!("Text(this.text_{}).fontSize(20)", safe)
    } else {
        format!("Text({}).fontSize(20)", arkts_string_lit(content))
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
fn emit_stack(
    arkui_kind: &str,
    args: &[Expr],
    bindings: &HashMap<LocalId, Expr>,
    depth: usize,
    callbacks: &mut Vec<Expr>,
    text_slots: &mut Vec<TextSlot>,
) -> String {
    // First-arg shape detection — same logic as lower_call/native.rs:91.
    let (spacing, children_idx) = match args.first() {
        Some(Expr::Array(_)) => (8.0, 0),
        Some(Expr::Number(n)) => (*n, 1),
        Some(Expr::Integer(n)) => (*n as f64, 1),
        _ => (8.0, 0),
    };

    let children = match args.get(children_idx) {
        Some(Expr::Array(items)) => items
            .iter()
            .map(|child| emit_widget(child, bindings, depth + 1, callbacks, text_slots))
            .collect::<Vec<_>>(),
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
fn emit_scrollview(
    args: &[Expr],
    bindings: &HashMap<LocalId, Expr>,
    depth: usize,
    callbacks: &mut Vec<Expr>,
    text_slots: &mut Vec<TextSlot>,
) -> String {
    let inner_indent = "    ".repeat(depth + 2);
    let mid_indent = "    ".repeat(depth + 1);
    let outer_indent = "    ".repeat(depth);

    let children: Vec<String> = match args.first() {
        Some(Expr::Array(items)) => items
            .iter()
            .map(|c| emit_widget(c, bindings, depth + 2, callbacks, text_slots))
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
fn emit_lazy_vstack(
    args: &[Expr],
    bindings: &HashMap<LocalId, Expr>,
    depth: usize,
    callbacks: &mut Vec<Expr>,
    text_slots: &mut Vec<TextSlot>,
) -> String {
    let inner_indent = "    ".repeat(depth + 1);
    let outer_indent = "    ".repeat(depth);

    let children: Vec<String> = match args.first() {
        Some(Expr::Array(items)) => items
            .iter()
            .map(|c| emit_widget(c, bindings, depth + 1, callbacks, text_slots))
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
        "// LazyVStack: rendered eagerly as Column. Real lazy support needs\n\
         {outer}// LazyForEach + IDataSource (Phase 2 v5 follow-up).\n\
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
fn emit_section(
    args: &[Expr],
    bindings: &HashMap<LocalId, Expr>,
    depth: usize,
    callbacks: &mut Vec<Expr>,
    text_slots: &mut Vec<TextSlot>,
) -> String {
    let title = first_string_arg(args).unwrap_or_default();

    let inner_indent = "    ".repeat(depth + 1);
    let outer_indent = "    ".repeat(depth);

    let children: Vec<String> = match args.get(1) {
        Some(Expr::Array(items)) => items
            .iter()
            .map(|c| emit_widget(c, bindings, depth + 1, callbacks, text_slots))
            .collect(),
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
