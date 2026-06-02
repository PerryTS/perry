//! JSX-intrinsic rewriting for `perry/tui` built-in widgets (#689).
//!
//! Extracted from `lower_call.rs` (#1099, part of #1097) — pure move,
//! no behavior change. Decodes the HIR JSX-intrinsic sentinel and
//! rewrites `jsx(Box|Text, props)` into the equivalent
//! `lower_native_method_call` dispatch.

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::FnCtx;
use crate::lower_call::lower_native_method_call;

/// Sentinel prefix the HIR's `lower_jsx_element_name` stamps onto an
/// `ExternFuncRef` when a JSX element name (e.g. `<Box>`) resolves to a
/// function imported from a native Perry module. Format:
/// `__perry_jsx_intrinsic::<module>::<method>__`.
///
/// Mirrors the producer in `crates/perry-hir/src/jsx.rs`. See
/// `try_rewrite_perry_tui_jsx_intrinsic` for the codegen consumer.
const JSX_INTRINSIC_SENTINEL_PREFIX: &str = "__perry_jsx_intrinsic::";
const JSX_INTRINSIC_SENTINEL_SUFFIX: &str = "__";

/// Decode an ExternFuncRef name into `(module, method)` if it carries
/// the JSX-intrinsic sentinel. Returns `None` for anything else — that's
/// the signal to fall back to the generic `js_jsx` runtime adapter.
fn decode_jsx_intrinsic_sentinel(name: &str) -> Option<(&str, &str)> {
    let inner = name
        .strip_prefix(JSX_INTRINSIC_SENTINEL_PREFIX)?
        .strip_suffix(JSX_INTRINSIC_SENTINEL_SUFFIX)?;
    let (module, method) = inner.rsplit_once("::")?;
    Some((module, method))
}

/// Pop the `"children"` field out of a JSX props object literal,
/// returning the remaining props and the children expression (if any).
/// JSX props arrive as `Expr::Object(fields)` from
/// `crates/perry-hir/src/jsx.rs`; non-Object shapes (e.g. `Expr::Null`
/// when an element has no attrs and no children) yield empty fields and
/// a None children expression.
fn split_children_from_props(props: &Expr) -> (Vec<(String, Expr)>, Option<Expr>) {
    let fields: Vec<(String, Expr)> = match props {
        Expr::Object(fields) => fields.clone(),
        _ => Vec::new(),
    };
    let mut other: Vec<(String, Expr)> = Vec::with_capacity(fields.len());
    let mut children: Option<Expr> = None;
    for (k, v) in fields.into_iter() {
        if k == "children" {
            children = Some(v);
        } else {
            other.push((k, v));
        }
    }
    (other, children)
}

/// JSX intrinsic rewriter for `perry/tui` built-in widgets (issue #689).
///
/// The HIR's JSX lowering (`crates/perry-hir/src/jsx.rs`) emits
/// `Call { callee: ExternFuncRef("jsx"|"jsxs"), args: [type, props] }`
/// for every JSX element. When the JSX name (`<Box>` / `<Text>`)
/// resolves to a native-module-imported intrinsic, the HIR stamps the
/// type ExternFuncRef with a sentinel name encoding the source module
/// + exported method — see `JSX_INTRINSIC_SENTINEL_PREFIX`.
///
/// This function decodes the sentinel and, for `perry/tui::Box` and
/// `perry/tui::Text` specifically, rewrites the JSX-shaped call into a
/// direct `NativeMethodCall` that goes through
/// `lower_native_method_call` — i.e. the exact same lowering as the
/// function-call form `Box(opts, children)` / `Text(content, opts)`.
/// The user-component path (`<App />` where `App` is a closure) is
/// unaffected because that lowers as `ExternFuncRef("App")` /
/// `LocalGet(...)`, neither of which carries the intrinsic sentinel.
///
/// Returns `Ok(Some(call_value))` if a rewrite happened, `Ok(None)` to
/// fall through to the generic `js_jsx` adapter.
///
/// Coverage today (v1):
/// - `perry/tui::Box` — pops `children` out of props, packs remaining
///   props into a style options object, wraps a single child in a
///   one-element array, dispatches `Box(opts, children)`.
/// - `perry/tui::Text` — pops `children` out of props as the content
///   string, dispatches `Text(content, opts)` for the styled form or
///   `Text(content)` for the bare form.
///
/// Follow-up scope (#689): Spacer, Input, Spinner, List, Select,
/// ProgressBar, Table, Tabs, TextArea. All currently fall through to
/// `js_jsx`; the runtime returns `undefined` for those unrecognised intrinsic
/// sentinels until the rewriter is extended.
pub(super) fn try_rewrite_perry_tui_jsx_intrinsic(
    ctx: &mut FnCtx<'_>,
    _is_jsxs: bool,
    args: &[Expr],
) -> Result<Option<String>> {
    // `jsx(type, props)` always arrives with exactly two args. Anything
    // else is unexpected — let it fall through to the runtime adapter
    // so the user sees the diagnostic from the failure-mode path
    // instead of a silent rewrite.
    let (type_arg, props_arg) = match args {
        [t, p] => (t, p),
        _ => return Ok(None),
    };

    let Expr::ExternFuncRef {
        name: type_name, ..
    } = type_arg
    else {
        return Ok(None);
    };
    let Some((module, method)) = decode_jsx_intrinsic_sentinel(type_name) else {
        return Ok(None);
    };

    // V1: only `perry/tui::Box` and `perry/tui::Text` are rewritten.
    // Other native modules / other perry/tui intrinsics fall through.
    if module != "perry/tui" {
        return Ok(None);
    }

    match method {
        "Box" => Some(rewrite_jsx_box(ctx, props_arg)).transpose(),
        "Text" => Some(rewrite_jsx_text(ctx, props_arg)).transpose(),
        _ => Ok(None),
    }
}

/// Rewrite `jsx(Box, { …props…, children })` into the `Box(opts, children_array)`
/// function-call form, then dispatch through `lower_native_method_call`.
/// `<Box>` with no children lowers to `Box(opts)` (1-arg shape, which
/// the dispatch table handles).
fn rewrite_jsx_box(ctx: &mut FnCtx<'_>, props_arg: &Expr) -> Result<String> {
    let (other_props, children_opt) = split_children_from_props(props_arg);

    // Build the children-array arg.
    //
    // JSX's single-child path emits `children: <expr>` directly (NOT
    // wrapped in an array); the multi-child path emits
    // `children: Expr::Array([...])`. Three children shapes to
    // distinguish:
    //
    // 1. `Expr::Array([...])` — multi-child JSX. Pass through; the
    //    Box dispatch iterates the elements at compile time.
    // 2. `Expr::Call { callee: ExternFuncRef("jsx"|"jsxs") }` — single
    //    JSX widget child (e.g. `<Box><Text>hi</Text></Box>`). Wrap
    //    in `Array([child])` so the Box dispatch sees one widget at
    //    compile time. Without the wrap, Box would route this through
    //    `js_perry_tui_box_add_children_array` and crash trying to
    //    read array headers from a widget handle.
    // 3. Anything else — a runtime expression that evaluates to a
    //    widget *array* (e.g. `<Box>{labels.map(l => <Text>{l}</Text>)}</Box>`).
    //    Pass through so Box uses its runtime
    //    `js_perry_tui_box_add_children_array` iterator. This matches
    //    React/ink semantics: `{arr}` as a single child is flattened.
    //    A `LocalGet` holding a single widget value is the one shape
    //    this heuristic gets wrong; users with that pattern should
    //    wrap explicitly: `<Box>{[w]}</Box>`.
    let children_array_opt: Option<Expr> = children_opt.map(|c| match &c {
        Expr::Array(_) => c,
        Expr::Call { callee, .. } if is_jsx_call_callee(callee) => Expr::Array(vec![c]),
        _ => c,
    });

    let opts_expr = Expr::Object(other_props);

    let synth_args: Vec<Expr> = match children_array_opt {
        Some(children) => vec![opts_expr, children],
        None => vec![opts_expr],
    };

    lower_native_method_call(ctx, "perry/tui", None, "Box", None, &synth_args)
}

/// True if `callee` is the HIR's marker for a JSX call (either the
/// generic `jsx`/`jsxs` adapter, or one of the intrinsic-sentinel
/// `ExternFuncRef`s the HIR stamps onto perry/tui widget tags). Used
/// by `rewrite_jsx_box` to recognise single-widget children that need
/// to be wrapped in a one-element array before reaching the Box
/// dispatch.
fn is_jsx_call_callee(callee: &Expr) -> bool {
    matches!(
        callee,
        Expr::ExternFuncRef { name, .. }
            if name == "jsx"
                || name == "jsxs"
                || name.starts_with(JSX_INTRINSIC_SENTINEL_PREFIX)
    )
}

/// Rewrite `jsx(Text, { …style…, children })` into the `Text(content, opts)`
/// function-call form, then dispatch through `lower_native_method_call`.
/// Single-child `<Text>{x}</Text>` passes the child as the content.
/// Multi-child `<Text>count: {n}</Text>` concatenates the children with
/// `+` so the runtime gets a single string content — ink behaves the
/// same way (children are folded into one rendered string).
fn rewrite_jsx_text(ctx: &mut FnCtx<'_>, props_arg: &Expr) -> Result<String> {
    let (other_props, children_opt) = split_children_from_props(props_arg);

    // Pick the content expression.
    // - `None` (no children) → empty string. Matches `<Text></Text>`
    //   behaviour in ink (renders an empty line).
    // - `Some(Array([...]))` from `jsxs` (>1 child) → fold the elements
    //   together with binary `+`. Perry's runtime promotes both sides
    //   to strings when either operand is a string, so `"count: " + n`
    //   yields the same value the function-call form
    //   `Text("count: " + n)` would.
    // - `Some(_)` otherwise → use the value directly.
    let content_expr: Expr = match children_opt {
        Some(Expr::Array(elems)) => fold_string_concat(elems),
        Some(other) => other,
        None => Expr::String(String::new()),
    };

    // If there are style props, dispatch the 2-arg styled form;
    // otherwise drop into the 1-arg bare form so the dispatch table
    // routes straight to `js_perry_tui_text`.
    let synth_args: Vec<Expr> = if other_props.is_empty() {
        vec![content_expr]
    } else {
        vec![content_expr, Expr::Object(other_props)]
    };

    lower_native_method_call(ctx, "perry/tui", None, "Text", None, &synth_args)
}

/// Left-fold a list of expressions into `a + b + c + …`. Used by
/// `rewrite_jsx_text` to flatten multi-child `<Text>foo {x} bar</Text>`
/// into a single content string before handing off to the perry/tui
/// Text dispatch. Empty input returns the empty string literal so the
/// runtime renders an empty line (parity with ink).
fn fold_string_concat(mut elems: Vec<Expr>) -> Expr {
    if elems.is_empty() {
        return Expr::String(String::new());
    }
    let mut iter = elems.drain(..);
    let mut acc = iter.next().unwrap();
    for next in iter {
        acc = Expr::Binary {
            op: perry_hir::BinaryOp::Add,
            left: Box::new(acc),
            right: Box::new(next),
        };
    }
    acc
}
