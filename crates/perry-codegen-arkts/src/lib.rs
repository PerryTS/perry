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
//! Phase 2 v1.5 scope:
//! - `App({body: <expr>})` extraction
//! - `Text(literal)` → `Text('lit').fontSize(20)`
//! - `VStack([...], spacing?)` → `Column({space: <spacing>}) { ... }`
//! - `HStack([...], spacing?)` → `Row({space: <spacing>}) { ... }`
//! - `Button(label, onPress)` → `Button('label')` (callback dropped — see Reactivity caveat)
//! - `TextField(placeholder, onChange)` → `TextInput({placeholder: 'hint'})`
//! - `Toggle(label, onChange)` → label rendered as Text + ArkUI Toggle in a Row
//! - `Slider(min, max, onChange)` → `Slider({min, max, value: min})`
//! - `Spacer()` → `Blank()`
//! - `Divider()` → `Divider()`
//! - LocalGet escape: `let x = Text("hi"); App({body: x})` follows the
//!   binding back to its init expression for any read-only top-level local.
//! - String / numeric / boolean literal arg coverage; closure args are silently
//!   dropped (no reactivity bridge yet).
//!
//! Reactivity caveat: ArkUI's `@State` / `@Link` decorators handle UI
//! reactivity natively, but Perry's runtime `State<T>` lives in the .so
//! and doesn't share memory with the ArkTS heap. State binding across the
//! NAPI boundary needs a poll/push mechanism that's deferred to a later
//! phase. Today's emitter handles static UI shapes only — Button / Toggle /
//! Slider / TextField widgets render but their event callbacks don't fire
//! Perry TS code yet.

use anyhow::Result;
use perry_hir::ir::{Class, Expr, Module, Stmt};
use std::collections::HashMap;

// LocalId is `u32` upstream; re-import directly so we don't carry a
// transitive dep on perry-types just for the type alias.
type LocalId = u32;

/// Walk `module.init` for the first `App({...})` call from `perry/ui`,
/// emit the corresponding ArkUI `pages/Index.ets`, AND **destructively
/// strip the App call from the HIR** so the LLVM backend doesn't emit
/// `perry_ui_*` FFI calls that would be unresolved on the OHOS target
/// (no `perry-ui-harmonyos` crate exists — UI is rendered declaratively
/// from the emitted `.ets`, not imperatively from native code).
///
/// Returns `Ok(None)` if the module doesn't use `perry/ui App` (the caller
/// should fall through to the blank EntryAbility-only stub; HIR is
/// untouched). Returns `Ok(Some(ets_source))` for static-UI programs where
/// we successfully harvested the widget tree.
pub fn emit_index_ets(module: &mut Module) -> Result<Option<String>> {
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
    let widget_arkui = emit_widget(&body_expr, &bindings, 0);
    Ok(Some(wrap_index_page(&widget_arkui)))
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
/// indentation when emitting nested children (Column/Row contents).
/// Unrecognized widgets degrade to a comment + a placeholder Text — never
/// errors out, since emit-time errors would leave the user without any UI.
fn emit_widget(expr: &Expr, bindings: &HashMap<LocalId, Expr>, depth: usize) -> String {
    let resolved = resolve(expr, bindings);
    match &resolved {
        Expr::NativeMethodCall {
            module: m,
            method,
            args,
            ..
        } if m == "perry/ui" => match method.as_str() {
            "Text" => emit_text(args),
            "VStack" => emit_stack("Column", args, bindings, depth),
            "HStack" => emit_stack("Row", args, bindings, depth),
            "Button" => emit_button(args),
            "TextField" => emit_textfield(args),
            "Toggle" => emit_toggle(args),
            "Slider" => emit_slider(args),
            "Spacer" => "Blank()".to_string(),
            "Divider" => "Divider()".to_string(),
            other => format!(
                "// unsupported perry/ui widget: {} (Phase 2 v1.5)\n\
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

/// `Text("hi")` → `Text('hi').fontSize(20)`. Non-string-literal args fall
/// back to a placeholder so unsupported shapes don't break the build.
fn emit_text(args: &[Expr]) -> String {
    if let Some(Expr::String(s)) = args.first() {
        format!("Text({}).fontSize(20)", arkts_string_lit(s))
    } else {
        "Text('[non-literal Text arg]').fontSize(20).fontColor('#888888')".to_string()
    }
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
            .map(|child| emit_widget(child, bindings, depth + 1))
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

/// `Button("label", onPress)` → `Button('label')`. The closure is dropped
/// — ArkUI's `.onClick(...)` would need a way to call back into Perry's
/// .so, which we don't have without a NAPI reactivity bridge.
fn emit_button(args: &[Expr]) -> String {
    let label = first_string_arg(args).unwrap_or_else(|| "Button".to_string());
    format!("Button({}).fontSize(16)", arkts_string_lit(&label))
}

/// `TextField(placeholder, onChange)` → `TextInput({placeholder: 'hint'})`.
/// Closure dropped (same reactivity-bridge caveat as Button).
fn emit_textfield(args: &[Expr]) -> String {
    let placeholder = first_string_arg(args).unwrap_or_default();
    format!(
        "TextInput({{ placeholder: {} }})",
        arkts_string_lit(&placeholder)
    )
}

/// `Toggle(label, onChange)` → label as a sibling Text + ArkUI's Toggle in
/// a Row. Visual approximation; reactivity bridge is the future work.
fn emit_toggle(args: &[Expr]) -> String {
    let label = first_string_arg(args).unwrap_or_default();
    if label.is_empty() {
        "Toggle({ type: ToggleType.Switch, isOn: false })".to_string()
    } else {
        format!(
            "Row({{ space: 8 }}) {{\n\
             \x20\x20\x20\x20Text({}).fontSize(16)\n\
             \x20\x20\x20\x20Toggle({{ type: ToggleType.Switch, isOn: false }})\n\
             }}",
            arkts_string_lit(&label)
        )
    }
}

/// `Slider(min, max, onChange)` → `Slider({min, max, value: min, step: 1})`.
fn emit_slider(args: &[Expr]) -> String {
    let min = numeric_arg(args, 0).unwrap_or(0.0);
    let max = numeric_arg(args, 1).unwrap_or(100.0);
    format!(
        "Slider({{ value: {min}, min: {min}, max: {max}, step: 1, style: SliderStyle.OutSet }})",
        min = fmt_num(min),
        max = fmt_num(max),
    )
}

/// Wrap a widget body expression in a complete ArkUI `@Entry @Component
/// struct Index { build() { Column() { ... } } }` page.
fn wrap_index_page(widget_body: &str) -> String {
    let indented = widget_body
        .lines()
        .map(|line| format!("            {}", line))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "// Auto-generated by Perry (perry-codegen-arkts) — do not edit.\n\
         // Regenerated every `perry compile --target harmonyos`.\n\
         //\n\
         // Source of truth is the `App({{body: ...}})` call in your\n\
         // TypeScript entry. Edit there; this file is overwritten.\n\
         @Entry\n\
         @Component\n\
         struct Index {{\n\
         \x20\x20\x20\x20build() {{\n\
         \x20\x20\x20\x20\x20\x20\x20\x20Column() {{\n\
         {body}\n\
         \x20\x20\x20\x20\x20\x20\x20\x20}}\n\
         \x20\x20\x20\x20\x20\x20\x20\x20.width('100%')\n\
         \x20\x20\x20\x20\x20\x20\x20\x20.height('100%')\n\
         \x20\x20\x20\x20\x20\x20\x20\x20.justifyContent(FlexAlign.Center)\n\
         \x20\x20\x20\x20}}\n\
         }}\n",
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
        let ets = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(ets.contains("Text('hi').fontSize(20)"));
        assert!(matches!(m.init[0], Stmt::Expr(Expr::Number(_))));
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
        let ets = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(ets.contains("Column({ space: 8 })"));
        assert!(ets.contains("Text('a').fontSize(20)"));
        assert!(ets.contains("Text('b').fontSize(20)"));
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
        let ets = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(ets.contains("Column({ space: 16 })"));
    }

    #[test]
    fn hstack_emits_row() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "HStack",
            vec![Expr::Array(vec![nmc("Spacer", vec![])])],
        )));
        let ets = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(ets.contains("Row({ space: 8 })"));
        assert!(ets.contains("Blank()"));
    }

    #[test]
    fn button_label_only() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Button",
            vec![
                Expr::String("Save".into()),
                Expr::Number(0.0), // would be a closure in real code; we ignore it
            ],
        )));
        let ets = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(ets.contains("Button('Save').fontSize(16)"));
    }

    #[test]
    fn textfield_placeholder() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "TextField",
            vec![Expr::String("Search…".into()), Expr::Number(0.0)],
        )));
        let ets = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(ets.contains("TextInput({ placeholder: 'Search…' })"));
    }

    #[test]
    fn toggle_with_label_emits_row() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Toggle",
            vec![Expr::String("Notifications".into()), Expr::Number(0.0)],
        )));
        let ets = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(ets.contains("Row({ space: 8 })"));
        assert!(ets.contains("Text('Notifications')"));
        assert!(ets.contains("Toggle({ type: ToggleType.Switch, isOn: false })"));
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
        let ets = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(ets.contains("min: 0"));
        assert!(ets.contains("max: 100"));
    }

    #[test]
    fn divider_no_args() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc("Divider", vec![])));
        let ets = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(ets.contains("Divider()"));
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
        let ets = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(ets.contains("Column({ space: 8 })"));
        assert!(ets.contains("Row({ space: 8 })"));
        assert!(ets.contains("Text('L')"));
        assert!(ets.contains("Text('R')"));
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
        let ets = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(ets.contains("Text('via let')"));
    }

    #[test]
    fn unsupported_widget_degrades_with_comment_not_error() {
        let mut m = empty_module();
        m.init
            .push(app_with_body(nmc("Picker", vec![Expr::Array(vec![])])));
        let ets = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(ets.contains("// unsupported perry/ui widget: Picker"));
        assert!(ets.contains("Text('[unsupported: Picker]')"));
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
