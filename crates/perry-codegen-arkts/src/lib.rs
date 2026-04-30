//! ArkUI/ArkTS code generation for Perry --target harmonyos.
//!
//! HarmonyOS NEXT renders UI declaratively from `.ets` files annotated with
//! `@Entry @Component struct ... { build() { ... } }`. Perry's `perry/ui`
//! surface (`App({body: Text("hi")})`) is normally lowered to native FFI
//! calls (perry_ui_app_create / perry_ui_app_set_body / perry_ui_app_run) on
//! iOS / macOS / Android / Linux / Windows — backed by perry-ui-* crates that
//! call into UIKit / AppKit / GTK4 / Win32 imperatively.
//!
//! HarmonyOS doesn't fit that imperative model: ArkTS owns the UI tree, not
//! native code. So instead of routing `App({...})` through FFI, this crate
//! walks the HIR pre-codegen, harvests the `perry/ui` widget tree, and emits
//! it as a real ArkUI `pages/Index.ets` file. The compiled `.so` then has
//! no UI calls at all — Perry's `main()` runs once at NAPI startup for any
//! non-UI logic, and ArkUI declaratively renders the harvested tree.
//!
//! Phase 2 v1 scope (this PR): handles `App({body: Text(literal)})`. Wider
//! widget support (VStack, HStack, Button, TextField, State<T> reactivity,
//! …) extends the `emit_widget` arm matching by widget name. Each new
//! widget is one match arm + one ArkUI shape line.
//!
//! Reactivity caveat: ArkUI's `@State` / `@Link` decorators handle UI
//! reactivity natively, but Perry's runtime `State<T>` lives in the .so
//! and doesn't share memory with the ArkTS heap. State binding across the
//! NAPI boundary needs a poll/push mechanism that's deferred to a later
//! phase. Today's emitter handles static UI only.

use anyhow::Result;
use perry_hir::ir::{Class, Expr, Module, Stmt};

/// Walk `module.init` for the first `App({...})` call from `perry/ui`,
/// emit the corresponding ArkUI `pages/Index.ets`, AND **destructively
/// strip the App call from the HIR** so the LLVM backend doesn't emit
/// `perry_ui_app_create` / `perry_ui_app_set_body` / `perry_ui_app_run`
/// FFI calls that would be unresolved on the OHOS target (no
/// `perry-ui-harmonyos` crate exists — UI is rendered declaratively from
/// the emitted `.ets`, not imperatively from native code).
///
/// Returns `Ok(None)` if the module doesn't use `perry/ui App` (the caller
/// should fall through to the blank EntryAbility-only stub; HIR is
/// untouched). Returns `Ok(Some(ets_source))` for static-UI programs where
/// we successfully harvested the widget tree (HIR has been mutated — the
/// `App({...})` `Stmt::Expr` is replaced with `Stmt::Expr(Expr::Number(0.0))`
/// to keep statement count stable for any debug-info indexing). Returns
/// `Err(...)` only on internal bugs.
///
/// Phase 2 v1 caveat: the body walk only follows perry/ui calls **directly
/// inline** inside `App({body: ...})`. If a user binds a widget to a local
/// (`let t = Text("hi"); App({body: t})`), the LocalGet escape isn't
/// followed and `t`'s nested calls survive into codegen as unresolved
/// FFIs. Document the inline-body restriction; broader walking comes later.
pub fn emit_index_ets(module: &mut Module) -> Result<Option<String>> {
    // Snapshot the class table BEFORE the &mut borrow on init so we can
    // look up __AnonShape_* classes (Perry's closed-shape object-literal
    // optimization, v0.5.337+) without aliasing &mut module.
    let classes = module.classes.clone();
    let Some(body_expr) = find_and_strip_app(&mut module.init, &classes) else {
        return Ok(None);
    };
    let widget_arkui = emit_widget(&body_expr);
    Ok(Some(wrap_index_page(&widget_arkui)))
}

/// Find the first top-level `App({body: <expr>})` call in `module.init`,
/// **return its body by-value**, and replace the entire statement with a
/// no-op `Stmt::Expr(Expr::Number(0.0))`. Other statements are untouched
/// so logic before/after `App(...)` still runs in `perryEntry.run()`.
///
/// Two object-literal shapes are accepted:
///
///  1. `Expr::Object(Vec<(String, Expr)>)` — used for spread-bearing or
///     dynamic-key objects. Direct lookup by key.
///  2. `Expr::New { class_name: "__AnonShape_<N>", args }` — Perry's
///     closed-shape optimization (v0.5.337+) where `App({title: "X",
///     body: ...})` lowers to `new __AnonShape_0("X", ...)` with field
///     order matching the synthesized class's `fields[]` declaration.
///     We look up the class in `module.classes`, find the index of the
///     `body` field, and return `args[body_index]`.
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

/// Emit an ArkUI expression for a perry/ui widget call. Returns the inner
/// `build()`-block content (no wrapping component). Unrecognized widgets
/// degrade to a comment + a placeholder Text — never errors out, since
/// emit-time errors would leave the user without any UI at all.
fn emit_widget(expr: &Expr) -> String {
    match expr {
        Expr::NativeMethodCall {
            module: m,
            method,
            args,
            ..
        } if m == "perry/ui" => match method.as_str() {
            "Text" => emit_text(args),
            other => format!(
                "// unsupported perry/ui widget: {} (Phase 2 v1 supports Text only)\n\
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

/// `Text("hello")` → `Text('hello').fontSize(20)`. Non-string-literal args
/// fall back to a placeholder so unsupported shapes don't break the build.
fn emit_text(args: &[Expr]) -> String {
    if let Some(Expr::String(s)) = args.first() {
        format!("Text({}).fontSize(20)", arkts_string_lit(s))
    } else {
        "Text('[non-literal Text arg]').fontSize(20).fontColor('#888888')".to_string()
    }
}

/// Wrap a widget body expression in a complete ArkUI `@Entry @Component
/// struct Index { build() { Column() { ... } } }` page. The `Column()`
/// + width/height/justify wrapping matches DevEco's stock Index.ets so
/// the emitted page lays out identically until a user supplies their own
/// container widget.
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

    #[test]
    fn emits_none_for_empty_module() {
        let mut m = empty_module();
        assert!(emit_index_ets(&mut m).unwrap().is_none());
    }

    #[test]
    fn emits_text_widget_and_strips_app_call() {
        let mut m = empty_module();
        m.init.push(Stmt::Expr(Expr::NativeMethodCall {
            module: "perry/ui".to_string(),
            class_name: None,
            object: None,
            method: "App".to_string(),
            args: vec![Expr::Object(vec![(
                "body".to_string(),
                Expr::NativeMethodCall {
                    module: "perry/ui".to_string(),
                    class_name: None,
                    object: None,
                    method: "Text".to_string(),
                    args: vec![Expr::String("hello phase 2".to_string())],
                },
            )])],
        }));
        let ets = emit_index_ets(&mut m).unwrap().expect("expected Index.ets");
        assert!(ets.contains("@Entry"));
        assert!(ets.contains("@Component"));
        assert!(ets.contains("struct Index"));
        assert!(ets.contains("Text('hello phase 2')"));
        assert!(ets.contains(".fontSize(20)"));
        // App call was stripped — the statement is now a no-op number expr
        assert_eq!(m.init.len(), 1);
        assert!(matches!(m.init[0], Stmt::Expr(Expr::Number(_))));
    }

    #[test]
    fn unsupported_widget_degrades_with_comment_not_error() {
        let mut m = empty_module();
        m.init.push(Stmt::Expr(Expr::NativeMethodCall {
            module: "perry/ui".to_string(),
            class_name: None,
            object: None,
            method: "App".to_string(),
            args: vec![Expr::Object(vec![(
                "body".to_string(),
                Expr::NativeMethodCall {
                    module: "perry/ui".to_string(),
                    class_name: None,
                    object: None,
                    method: "VStack".to_string(),
                    args: vec![],
                },
            )])],
        }));
        let ets = emit_index_ets(&mut m).unwrap().expect("expected Index.ets");
        assert!(ets.contains("// unsupported perry/ui widget: VStack"));
        assert!(ets.contains("Text('[unsupported: VStack]')"));
    }

    #[test]
    fn string_literal_escaping() {
        assert_eq!(arkts_string_lit("hi"), "'hi'");
        assert_eq!(arkts_string_lit("he's there"), "'he\\'s there'");
        assert_eq!(arkts_string_lit("a\\b"), "'a\\\\b'");
        assert_eq!(arkts_string_lit("line1\nline2"), "'line1\\nline2'");
    }
}
