//! Small predicate / resolution helpers used by the PropertyGet method-call
//! dispatch tower (`try_lower_property_get_method_call`). Pure code move from
//! `property_get.rs` — no behavior change.

use perry_hir::Expr;

use crate::expr::FnCtx;
use crate::type_analysis::receiver_class_name;

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

/// Array methods that are deliberately absent from `%TypedArray%.prototype`.
pub(crate) fn typed_array_lacks_array_method(name: &str) -> bool {
    matches!(
        name,
        "flat" | "flatMap" | "push" | "pop" | "shift" | "unshift" | "splice" | "toSpliced"
    )
}

/// Whether an unproven receiver's argument count can use the selected static
/// String lowering. Invalid counts must bypass the tag diamond because its
/// String arm is emitted eagerly and may reject them before runtime dispatch.
pub(crate) fn string_only_method_arity_ok(name: &str, argc: usize) -> bool {
    match name {
        "trim" | "trimStart" | "trimEnd" | "toLowerCase" | "toUpperCase" => argc == 0,
        "toLocaleLowerCase" | "toLocaleUpperCase" => argc <= 1,
        "split" | "substring" => argc <= 2,
        "substr" | "padStart" | "padEnd" => argc == 1 || argc == 2,
        "replaceAll" => argc == 2,
        "repeat" => argc == 1,
        "localeCompare" => argc <= 3,
        // These lowerers intentionally evaluate and ignore surplus args.
        "charAt" | "charCodeAt" | "codePointAt" => true,
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

/// The class that the namespace member `namespace.member` is, if it is one.
///
/// The answer is the namespace-scoped identity
/// (`namespace_member_class_key`), which the class registration records for
/// exactly the members that resolve to a class in the namespace's module. A
/// bare `class_ids[member]` hit says nothing about the namespace: it is
/// whichever class some other import of this module declares under the same
/// name. opencode's `Skill.Info.make(...)` (`Info` a schema value) matched
/// the imported config `class Info` that way and was lowered as a static call
/// on that class, whose miss path hands back the receiver.
///
/// Returns the bare member name, under which the class tables key the class,
/// when that name registers the very same class. A member class known only
/// under the scoped key keeps the generic method call.
pub(crate) fn namespace_member_class(
    class_ids: &std::collections::HashMap<String, u32>,
    namespace: &str,
    member: &str,
) -> Option<String> {
    let class_id = class_ids.get(&crate::namespace_member_class_key(namespace, member))?;
    (class_ids.get(member) == Some(class_id)).then(|| member.to_string())
}

/// Resolve the static-method receiver class through one of several shapes:
///   - `Expr::ClassRef(name)` — direct class literal.
///   - `Expr::ExternFuncRef { name }` whose name is a known class — a
///     cross-module class accessed via direct named import (#1787 / #321).
///   - `Expr::PropertyGet { object: ExternFuncRef(ns), property }` where
///     `ns.property` IS a class — a namespace import (`AST.Union.make(...)`),
///     decided by [`namespace_member_class`].
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
        Expr::PropertyGet {
            object, property, ..
        } => match object.as_ref() {
            Expr::ExternFuncRef {
                name: namespace, ..
            } => namespace_member_class(class_ids, namespace, property),
            _ => None,
        },
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
