// This module is part of the perry-codegen-arkts crate. It was
// mechanically split out of the former monolithic lib.rs (issue
// #1100). Pure code move — no logic changes.
#![allow(clippy::too_many_arguments)]
use crate::*;

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
pub(crate) fn emit_for_each(
    array: &Expr,
    callback: &Expr,
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
                lazy_sources,
                extras,
                mutations,
                None,
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
pub(crate) fn arkts_array_source(e: &Expr, bindings: &HashMap<LocalId, Expr>) -> String {
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
pub(crate) fn arkts_value_literal(e: &Expr) -> String {
    match e {
        Expr::String(s) => arkts_string_lit(s),
        Expr::Number(n) => fmt_num(*n),
        Expr::Integer(n) => format!("{}", n),
        Expr::Bool(b) => format!("{}", b),
        _ => "null".to_string(),
    }
}
