// This module is part of the perry-codegen-arkts crate. It was
// mechanically split out of the former monolithic lib.rs (issue
// #1100). Pure code move — no logic changes.
#![allow(clippy::too_many_arguments)]
use crate::*;

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
pub(crate) fn emit_button(args: &[Expr], callbacks: &mut Vec<Expr>) -> String {
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

/// `TextField(placeholder, onChange)` → `TextInput(...).onChange(...)`.
/// Phase 2 v2.5: when `onChange` is a closure, register it in the slot
/// table and emit an `onChange((value: string) => perryEntry.invokeCallback1(idx, value))`
/// handler that also drains toast + text-update queues.
pub(crate) fn emit_textfield(args: &[Expr], callbacks: &mut Vec<Expr>) -> String {
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
pub(crate) fn emit_toggle(args: &[Expr], callbacks: &mut Vec<Expr>) -> String {
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
pub(crate) fn emit_slider(args: &[Expr], callbacks: &mut Vec<Expr>) -> String {
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

/// `Picker(options, onChange)` → ArkUI `TextPicker({range, value: range[0]}).onChange(...)`.
/// Closure receives `(idx: number)` matching the perry-ui-* TS surface.
/// ArkUI's onChange has the shape `(value: string, index: number)` — we
/// forward only `index` since that's what the Perry callback expects.
/// Same drain pattern as Toggle/Slider.
pub(crate) fn emit_picker(args: &[Expr], callbacks: &mut Vec<Expr>) -> String {
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

/// Issue #475 — `Combobox(initial, onChange)` → ArkUI `Select([...])` with
/// `.value()` / `.selected()` / `.onSelect()`. ArkUI's Select takes a
/// `SelectOption[]` (each option is `{value: string}`); `.value()` sets
/// the currently displayed label; `.onSelect((index, value) => ...)` fires
/// when the user picks an item.
///
/// v1 limitation: runtime `comboboxAddItem(widget, value)` calls aren't
/// folded into the static options list — only the `initial` string seeds
/// the dropdown. The macOS path's NSComboBox completion behavior doesn't
/// have a direct ArkUI analog; Select shows the dropdown on tap, which
/// is acceptable for the v1 milestone (#475).
pub(crate) fn emit_combobox(args: &[Expr], callbacks: &mut Vec<Expr>) -> String {
    let initial = first_string_arg(args).unwrap_or_default();
    let initial_lit = arkts_string_lit(&initial);

    let onchange = match args.get(1) {
        Some(closure @ Expr::Closure { .. }) => {
            let idx = callbacks.len();
            callbacks.push(closure.clone());
            format!(
                ".onSelect((_index: number, value: string) => {{\n    \
                 perryEntry.invokeCallback1({}, value);\n    \
                 {drain}\
                 }})",
                idx,
                drain = drain_loop_body()
            )
        }
        _ => String::new(),
    };

    // ArkUI Select requires a non-empty options array. Seed it with the
    // initial value so the dropdown isn't empty on first render. Real
    // app code that calls `comboboxAddItem` repeatedly will need a v1.1
    // mutator-style fold (similar to widgetAddChild) — out of scope #475 v1.
    format!(
        "Select([{{ value: {init} }}]).selected(0).value({init}){onchange}",
        init = initial_lit,
        onchange = onchange,
    )
}

/// Issue #478 — `RichTextEditor(width, height, onChange)` → ArkUI
/// `RichEditor({controller})`. ArkUI's RichEditor takes a
/// `RichEditorController` for programmatic content manipulation and fires
/// `.aboutToIMEInput` / `.onIMEInputComplete` lifecycle callbacks.
///
/// v1 mapping decisions:
///   - `width` / `height` flow through to `.width()` / `.height()` modifiers.
///   - `onChange` fires from `.onIMEInputComplete` — ArkUI's closest analog
///     to NSTextView's didChange notification. Forwards the plain-text view
///     of the editor content via `invokeCallback1(idx, plainText)`.
///   - `richTextToggleBold` / `Italic` / `Underline` would map to
///     `RichEditorController.updateSpanStyle({textStyle: ...})` on a
///     stored controller. v1 emits the editor + the controller field;
///     the mutator dispatch for the toggles is a v1.1 follow-up so the
///     emitted ArkTS still compiles and the editor itself renders.
///   - `richTextSetString` / `richTextGetString` map to the controller's
///     `addTextSpan` / `getSpans`; `setHtml` / `getHtml` are still
///     TODOs (ArkUI RichEditor doesn't ship a 1:1 HTML round-trip).
pub(crate) fn emit_richtexteditor(args: &[Expr], callbacks: &mut Vec<Expr>) -> String {
    let width = numeric_arg(args, 0).unwrap_or(0.0);
    let height = numeric_arg(args, 1).unwrap_or(0.0);

    let onchange = match args.get(2) {
        Some(closure @ Expr::Closure { .. }) => {
            let idx = callbacks.len();
            callbacks.push(closure.clone());
            // RichEditor's onIMEInputComplete gives us a TextRange/value
            // shape; for v1 we forward an empty string placeholder so the
            // TS side sees a valid arg slot. Wiring the actual plain-text
            // payload requires reading the controller post-event — v1.1.
            format!(
                ".onIMEInputComplete(() => {{\n    \
                 perryEntry.invokeCallback1({}, '');\n    \
                 {drain}\
                 }})",
                idx,
                drain = drain_loop_body()
            )
        }
        _ => String::new(),
    };

    // Width / height modifiers only emitted when non-zero so a caller
    // passing 0 (TS default) doesn't zero out the editor's intrinsic size.
    let mut sizing = String::new();
    if width > 0.0 {
        sizing.push_str(&format!(".width({})", fmt_num(width)));
    }
    if height > 0.0 {
        sizing.push_str(&format!(".height({})", fmt_num(height)));
    }

    format!(
        "RichEditor({{ controller: new RichEditorController() }}){sizing}{onchange}",
        sizing = sizing,
        onchange = onchange,
    )
}
