// This module is part of the perry-codegen-arkts crate. It was
// mechanically split out of the former monolithic lib.rs (issue
// #1100). Pure code move — no logic changes.
#![allow(clippy::too_many_arguments)]
use crate::*;

// ----- helpers -----

/// First arg matched as a string literal. Returns None if absent or
/// non-literal so callers can pick a sensible default.
pub(crate) fn first_string_arg(args: &[Expr]) -> Option<String> {
    match args.first() {
        Some(Expr::String(s)) => Some(s.clone()),
        _ => None,
    }
}

/// Get arg at `idx` as a Number, supporting both Integer and Number HIR
/// variants since perry-hir distinguishes them.
pub(crate) fn numeric_arg(args: &[Expr], idx: usize) -> Option<f64> {
    match args.get(idx) {
        Some(Expr::Number(n)) => Some(*n),
        Some(Expr::Integer(n)) => Some(*n as f64),
        _ => None,
    }
}

/// Like `numeric_arg`, but resolves `Expr::LocalGet(id)` through `bindings`
/// — e.g. `let size = 28; textSetFontSize(t, size)` resolves to `28`.
/// Returns `None` only when the chain bottoms out in a non-numeric leaf
/// (function call, prop-access, etc.) or hits an unbound local. Mango
/// uses this pattern heavily for theme-controlled values.
pub(crate) fn numeric_arg_resolved(
    args: &[Expr],
    idx: usize,
    bindings: &HashMap<LocalId, Expr>,
) -> Option<f64> {
    let mut cur = args.get(idx)?;
    // Bound the walk to avoid pathological binding cycles.
    for _ in 0..16 {
        match cur {
            Expr::Number(n) => return Some(*n),
            Expr::Integer(n) => return Some(*n as f64),
            Expr::Bool(true) => return Some(1.0),
            Expr::Bool(false) => return Some(0.0),
            Expr::LocalGet(id) => {
                cur = bindings.get(id)?;
            }
            // `cond ? a : b` — same heuristic as the widget Conditional
            // emitter and resolve_string_arg: const-fold the condition,
            // pick the resolved branch; default to then-branch when
            // unresolvable (Mango: `widgetSetWidth(logo, mobile ? 40 :
            // 44)` resolves through the ternary to the numeric leaf).
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                cur = match evaluate_condition(condition, bindings, &HashMap::new()) {
                    Some(false) => else_expr,
                    _ => then_expr,
                };
            }
            // HarmonyOS-stubbed perry/system functions return 0 (see
            // is_harmonyos_zero_fn). Treating them as 0 here makes
            // theme-color resolution like `txR = dark ? 0.91 : 0.17`
            // pick the else-branch (light mode) when dark is bound to
            // `isDarkMode()`.
            Expr::Call { callee, .. } => match callee.as_ref() {
                Expr::ExternFuncRef { name, .. } if is_harmonyos_zero_fn(name) => return Some(0.0),
                _ => return None,
            },
            Expr::NativeMethodCall { module, method, .. }
                if module == "perry/system" && is_harmonyos_zero_fn(method) =>
            {
                return Some(0.0);
            }
            _ => return None,
        }
    }
    None
}

/// Format a float as ArkTS source. Whole numbers emit without a decimal
/// (`8`, not `8.0`) to match ArkUI's idiomatic style.
pub(crate) fn fmt_num(n: f64) -> String {
    if n == n.trunc() && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

/// Escape a Rust string into an ArkTS single-quoted string literal.
/// ArkTS shares JS string-literal rules — escape backslash + single quote.
pub(crate) fn arkts_string_lit(s: &str) -> String {
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

/// Inspect the first arg of a perry/ui method call. If it's a
/// `LocalGet(id)` (the canonical "mutate a widget bound to a local"
/// shape), return the LocalId. Anything else (transient widget without
/// a binding, complex expression) returns None and the mutator is
/// dropped — the user code couldn't mutate something un-named anyway.
pub(crate) fn mutator_target_local_id(args: &[Expr]) -> Option<LocalId> {
    match args.first() {
        Some(Expr::LocalGet(id)) => Some(*id),
        _ => None,
    }
}

/// Issue #479 — extract a plain-text payload from a tooltip content
/// widget expression. The harvest can fold runtime-built widget trees
/// into ArkUI source via emit_widget, but the modifier pre-walk runs
/// before that pass and only has access to `bindings`. For v1 we
/// recognize the canonical shape `Text(literal-string)`, both as a
/// direct call and as a `LocalGet(id)` that resolves through bindings
/// to one. Anything else (HStack/VStack content, dynamic strings,
/// closure-captured locals) returns None and the caller falls back
/// to a comment.
pub(crate) fn resolve_tooltip_text(
    expr: &Expr,
    bindings: &HashMap<LocalId, Expr>,
) -> Option<String> {
    let mut cur = expr;
    for _ in 0..16 {
        match cur {
            Expr::NativeMethodCall {
                module,
                method,
                args,
                ..
            } if module == "perry/ui" && method == "Text" => {
                if let Some(Expr::String(s)) = args.first() {
                    return Some(s.clone());
                }
                return None;
            }
            Expr::LocalGet(id) => {
                cur = bindings.get(id)?;
            }
            _ => return None,
        }
    }
    None
}

/// Build the `.backgroundColor('rgba(R, G, B, A)')` modifier string from
/// the 4 channel args of a `widgetSetBackgroundColor(w, r, g, b, a)` call.
/// Channels are 0..1 floats matching the perry-ui-* TS surface.
pub(crate) fn mutator_background_color(
    args: &[Expr],
    bindings: &HashMap<LocalId, Expr>,
) -> Option<String> {
    // Resolve through bindings so theme-bound calls work — Mango's
    // `widgetSetBackgroundColor(btn, moR, moG, moB, 1.0)` where moR/G/B
    // are const-bound brand-color numbers needed `numeric_arg_resolved`,
    // not the literal-only `numeric_arg`.
    let r = numeric_arg_resolved(args, 0, bindings)?;
    let g = numeric_arg_resolved(args, 1, bindings)?;
    let b = numeric_arg_resolved(args, 2, bindings)?;
    let a = numeric_arg_resolved(args, 3, bindings).unwrap_or(1.0);
    let r255 = (r * 255.0).round() as i64;
    let g255 = (g * 255.0).round() as i64;
    let b255 = (b * 255.0).round() as i64;
    Some(format!(
        ".backgroundColor('rgba({}, {}, {}, {})')",
        r255,
        g255,
        b255,
        fmt_num(a)
    ))
}

/// Heuristic: known widget-factory names that should NOT be flagged as
/// missed mutators. Kept loose — false positives only produce extra
/// comments, not bugs.
pub(crate) fn is_widget_factory(name: &str) -> bool {
    matches!(
        name,
        "App"
            | "Text"
            | "Button"
            | "VStack"
            | "HStack"
            | "ZStack"
            | "ScrollView"
            | "LazyVStack"
            | "Spacer"
            | "Divider"
            | "Image"
            | "ImageFile"
            | "ImageSymbol"
            | "TextField"
            | "TextArea"
            | "Toggle"
            | "Slider"
            | "Picker"
            | "Combobox"
            | "RichTextEditor"
            | "Calendar"
            | "ProgressView"
            | "Section"
            | "Tabs"
            | "Modal"
            | "Dialog"
            | "Menu"
            | "ContextMenu"
            | "Grid"
            | "NavStack"
            | "Chart"
            | "TreeView"
            | "TreeNode"
            | "showToast"
            | "setText"
            | "state"
            | "stateCreate"
    )
}

/// Stringify a condition expression for emission in an ArkUI
/// `if (<cond>)` predicate. Handles the canonical comparison + logical
/// shapes the harvest can statically rewrite, plus a few literal forms.
/// Falls back to a `true` predicate (so the then-branch always renders)
/// for shapes the emitter can't safely render.
/// Issue #410 — serialize a condition expression to ArkTS source. The
/// emitted string is interpolated into `if (...)` blocks (for conditional
/// AddChild mutations) and into `/* if (...) */` comment markers (for
/// conditional Modifier mutations) in the generated Index.ets. Two
/// invariants must hold for the emitted ArkTS to compile:
///
/// 1. **No `*/` substring anywhere in the returned string.** When the
///    caller wraps the result in `/* if ((<cond>)) */`, any `*/` inside
///    `<cond>` would close the outer comment early and leak the rest as
///    code (see #410 line-82 cascade). Every branch of this function is
///    audited to ensure that — string literals route through
///    `arkts_string_lit` (single-quoted, so `*/` can't appear unescaped),
///    operator strings come from a closed enum, and the bottom-fallback
///    returns `"true"` (literally — not `"true /* unsupported */"`).
///
/// 2. **No `__local_N` placeholders.** `Expr::LocalGet(id)` references
///    must resolve via `bindings` (top-level `let x = <init>` HIR shape)
///    to a real, ArkTS-bindable expression. If the local can't be
///    resolved (closure-captured, loop-mutated, or a `declare const`
///    without an `init`), we degrade gracefully to `"true"` — losing the
///    conditionality but keeping the build green. Compile-time platform
///    constants like `__platform__` are inlined as numeric literals via
///    the `compile_time_consts` map.
///
/// Issue #413 — defensive parenthesization on nested operators. When a
/// resolved binding contains a Binary/Logical/Unary expression and that
/// expression is the operand of an outer Binary/Logical/Unary, ArkTS's
/// precedence rules can invert the user's intent. Concretely,
/// `mobile = __platform__ === 1 || __platform__ === 2 || (!isIOS && x)`
/// inlined as `9 === 1 || 9 === 2 && !9 === 1 && true === 1` (where
/// `!9 === 1` parses as `(!9) === 1` instead of `!(9 === 1)`). The fix
/// is to wrap any non-leaf serialized operand in parentheses before
/// splicing into the parent operator string. Leaf shapes (literals,
/// LocalGet that resolved to a literal, PropertyGet) don't need wrapping.
/// On HarmonyOS, the v0.5.477 build.rs-generated stubs return 0/false
/// for every perry/system + perry/ui FFI symbol that's not implemented
/// natively. The harvest's constant folder treats calls to these
/// functions as Lit::Num(0.0) so theme-switching code like
/// `const dark = isDarkMode()` folds to `dark = false` at codegen
/// time, picking the light-mode branch. Without this, the unfoldable-
/// LocalGet heuristic-pick-then-branch fallback selects the dark-mode
/// branch and Mango renders translucent light-text-on-light-background.
pub(crate) fn is_harmonyos_zero_fn(name: &str) -> bool {
    matches!(
        name,
        "isDarkMode"
            | "getDeviceIdiom"
            | "getDeviceModel"
            | "getDeviceOSVersion"
            | "isHighContrast"
            | "isReducedMotion"
            | "getNotchHeight"
    )
}

/// Issue #410 — replace any `*/` substring with `*\u{200b}/` (an inserted
/// zero-width space) so the result can be safely spliced inside a `/* ... */`
/// block comment marker without closing the outer comment early. The
/// zero-width space renders invisibly in editor diagnostics so the comment
/// stays human-readable. Also handles the `*//` edge case (where two
/// adjacent close-comment markers would survive a single replacement).
pub(crate) fn sanitize_for_block_comment(s: &str) -> String {
    if !s.contains("*/") {
        return s.to_string();
    }
    s.replace("*/", "*\u{200b}/")
}

/// Multi-pass drain after a closure body returns. Used by Button.onClick
/// (Phase 2 v2) and Toggle/TextField/Slider.onChange (v2.5):
///   1. drainToast loop → promptAction.showToast({message})
///   2. drainTextUpdate loop → this.applyTextUpdate(id, value)
///   3. drainVisibilityUpdate loop → this.applyVisibilityUpdate(id, hidden)  [v3.5]
/// `invokeCallback` itself is emitted by the caller because it varies
/// (callN with N-arg widgets, plus ArkUI's per-widget onChange shape).
pub(crate) fn drain_loop_body() -> String {
    "let __t = perryEntry.drainToast();\n    \
     while (__t !== undefined) { \
     promptAction.showToast({ message: __t }); \
     __t = perryEntry.drainToast(); \
     }\n    \
     let __u = perryEntry.drainTextUpdate();\n    \
     while (__u !== undefined) { \
     this.applyTextUpdate(__u.id, __u.value); \
     __u = perryEntry.drainTextUpdate(); \
     }\n    \
     let __v = perryEntry.drainVisibilityUpdate();\n    \
     while (__v !== undefined) { \
     this.applyVisibilityUpdate(__v.id, __v.hidden); \
     __v = perryEntry.drainVisibilityUpdate(); \
     }\n    \
     let __c = perryEntry.drainContentViewUpdate();\n    \
     while (__c !== undefined) { \
     this.applyContentViewUpdate(__c.id, __c.view); \
     __c = perryEntry.drainContentViewUpdate(); \
     }\n  "
        .to_string()
}

/// Issue #413 — return the ArkUI cross-axis alignment enum name for the
/// stack target. Column (= VStack) takes `HorizontalAlign`; Row (=
/// HStack) takes `VerticalAlign`. Looks up the binding for the local
/// to discover the constructor; falls back to `HorizontalAlign` (the
/// VStack default) when the binding can't be resolved or doesn't name
/// a recognized stack constructor.
pub(crate) fn stack_axis_align_enum(
    target_id: LocalId,
    bindings: &HashMap<LocalId, Expr>,
) -> &'static str {
    let Some(init) = bindings.get(&target_id) else {
        return "HorizontalAlign";
    };
    if let Expr::NativeMethodCall {
        module: m, method, ..
    } = init
    {
        if m == "perry/ui" {
            return match method.as_str() {
                "HStack" => "VerticalAlign",
                _ => "HorizontalAlign",
            };
        }
    }
    "HorizontalAlign"
}
