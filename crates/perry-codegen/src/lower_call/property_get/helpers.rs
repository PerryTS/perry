//! Small predicate / resolution helpers used by the PropertyGet method-call
//! dispatch tower (`try_lower_property_get_method_call`). Pure code move from
//! `property_get.rs` — no behavior change.

use super::*;

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{lower_expr, nanbox_pointer_inline, nanbox_string_inline, unbox_to_i64, FnCtx};
use crate::lower_array_method::lower_array_method;
use crate::lower_string_method::{is_known_string_method_name, lower_string_method};
use crate::nanbox::double_literal;
use crate::type_analysis::{
    is_array_expr, is_global_constructor_expr, is_map_expr, is_native_module_dynamic_index,
    is_promise_expr, is_set_expr, is_string_expr, is_url_search_params_expr, receiver_class_name,
};
use crate::types::{DOUBLE, I32, I64};

/// Methods that exist on `Array.prototype` but NOT on `String.prototype`.
/// Used to keep the string-method dispatch from claiming a call site
/// like `(s | T[]).join(",")` where the static type is permissive
/// (Union with String — see `is_string_expr`'s Union arm) but the
/// method itself isn't part of the string surface. Falling through to
/// the runtime dispatcher (`js_native_call_method`) lets the actual
/// runtime shape pick the right path. Refs #2277.
pub(crate) fn is_array_only_method_name(name: &str) -> bool {
    matches!(
        name,
        // Mutating
        "push" | "pop" | "shift" | "unshift" | "splice" | "sort" | "reverse" | "fill" | "copyWithin"
        // Aggregation / iteration
        | "join" | "every" | "some" | "filter" | "map" | "forEach" | "reduce" | "reduceRight"
        | "find" | "findIndex" | "findLast" | "findLastIndex" | "flat" | "flatMap"
        | "keys" | "values" | "entries"
        // Immutable variants
        | "toReversed" | "toSorted" | "toSpliced" | "with"
    )
}

/// For the Any-typed-receiver string-method fallback only: is `argc` a
/// plausible argument count for the String.prototype builtin named
/// `name`? When a builtin-named method is invoked on a receiver that is
/// NOT provably a string (object literal, `any`, unknown) AND the arg
/// count can't match the String builtin's signature, the call is almost
/// certainly a user method that merely shares a name with a String
/// builtin — e.g. joi's `internals.trim(value, schema)` (#5271). Forcing
/// the String path there used to abort codegen with
/// "String.trim takes no args, got 2"; gating on arity here lets such
/// calls fall through to the runtime method dispatcher instead.
///
/// The accepted ranges mirror `lower_string_method`'s per-arm arity
/// guards. Char-access methods (`charAt`/`charCodeAt`/`codePointAt`)
/// ignore surplus args per spec, so any count is fine for them.
pub(crate) fn string_only_method_arity_ok(name: &str, argc: usize) -> bool {
    match name {
        // No-arg string transforms.
        "trim" | "trimStart" | "trimEnd" | "toLowerCase" | "toUpperCase" => argc == 0,
        // Locale-aware case folding: optional `locales`.
        "toLocaleLowerCase" | "toLocaleUpperCase" => argc <= 1,
        // split(separator?, limit?).
        "split" => argc <= 2,
        // substring(start?, end?).
        "substring" => argc <= 2,
        // substr(start, length?) — start is required.
        "substr" => argc == 1 || argc == 2,
        // replaceAll(search, replace).
        "replaceAll" => argc == 2,
        // padStart/padEnd(targetLength, padString?).
        "padStart" | "padEnd" => argc == 1 || argc == 2,
        // repeat(count).
        "repeat" => argc == 1,
        // localeCompare(that, locales?, options?).
        "localeCompare" => argc <= 3,
        // Char-access ignores extra args (still evaluated for side effects).
        "charAt" | "charCodeAt" | "codePointAt" => true,
        // Conservative default: methods reaching this gate but not listed
        // here keep their prior (already arity-checked) routing.
        _ => true,
    }
}

pub(crate) fn is_date_receiver(ctx: &FnCtx<'_>, object: &Expr) -> bool {
    matches!(object, Expr::DateNew(_))
        || receiver_class_name(ctx, object).as_deref() == Some("Date")
}

pub(crate) fn is_inherited_object_prototype_method(name: &str) -> bool {
    matches!(
        name,
        "hasOwnProperty"
            | "propertyIsEnumerable"
            | "isPrototypeOf"
            | "valueOf"
            // Annex B §B.2.2 legacy accessor helpers — inherited from
            // Object.prototype by every instance (incl. class instances).
            | "__defineGetter__"
            | "__defineSetter__"
            | "__lookupGetter__"
            | "__lookupSetter__"
    )
}

pub(crate) fn class_chain_has_field_named(
    ctx: &FnCtx<'_>,
    class_name: &str,
    property: &str,
) -> bool {
    let mut current = Some(class_name.to_string());
    while let Some(name) = current {
        let Some(class) = ctx.classes.get(&name) else {
            return true;
        };
        if class
            .fields
            .iter()
            .any(|field| field.key_expr.is_some() || (!field.is_private && field.name == property))
        {
            return true;
        }
        current = class.extends_name.clone();
    }
    false
}

/// Resolve the static-method receiver class through one of several shapes:
///   - `Expr::ClassRef(name)` — direct class literal.
///   - `Expr::ExternFuncRef { name }` whose name is a known class — a
///     cross-module class accessed via direct named import (#1787 / #321).
///   - `Expr::PropertyGet { object: ExternFuncRef, property }` whose property
///     is a known class — a namespace import (`AST.Union.make(...)`).
///   - `Expr::ClassExprFresh { template }` — a class-expression value (#1787).
///   - `Expr::LocalGet(id)` whose let-init was a ClassRef (the post-#912
///     `const Cls = make(); Cls.foo(...)` shape).
///   - `Expr::Call { callee: FuncRef(fid) }` where `fid` is a factory function
///     tagged via `func_returns_class`.
///   - `Expr::Sequence` whose trailing expression resolves to a class.
///
/// See `try_lower_static_dispatch` for the original narrative comments
/// motivating each shape (#687 / #915 / #1787 / #321).
pub(crate) fn resolve_static_dispatch_cls(
    expr: &Expr,
    local_id_to_name: &std::collections::HashMap<u32, String>,
    local_class_aliases: &std::collections::HashMap<String, String>,
    func_returns_class: &std::collections::HashMap<u32, String>,
    class_ids: &std::collections::HashMap<String, u32>,
) -> Option<String> {
    match expr {
        Expr::ClassRef(name) => Some(name.clone()),
        Expr::ExternFuncRef { name, .. } if class_ids.contains_key(name) => Some(name.clone()),
        Expr::PropertyGet { object, property }
            if matches!(object.as_ref(), Expr::ExternFuncRef { .. })
                && class_ids.contains_key(property) =>
        {
            Some(property.clone())
        }
        Expr::ClassExprFresh { template, .. } => Some(template.clone()),
        Expr::LocalGet(id) => local_id_to_name
            .get(id)
            .and_then(|name| local_class_aliases.get(name).cloned()),
        Expr::Call { callee, .. } => match callee.as_ref() {
            Expr::FuncRef(fid) => func_returns_class.get(fid).cloned(),
            _ => None,
        },
        Expr::Sequence(exprs) => exprs.last().and_then(|e| {
            resolve_static_dispatch_cls(
                e,
                local_id_to_name,
                local_class_aliases,
                func_returns_class,
                class_ids,
            )
        }),
        _ => None,
    }
}
