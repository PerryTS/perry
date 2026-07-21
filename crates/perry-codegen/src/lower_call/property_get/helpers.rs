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

/// True when `object`'s statically-known class (or an ancestor) defines its
/// OWN instance method, getter, or field named `name`. Keeps the static
/// String-method fast path from hijacking a user class member that merely
/// shares a `String.prototype` name. This matters most for the char-access methods
/// (`charAt`/`charCodeAt`/`codePointAt`): their arity gate can never
/// disambiguate a user method from the builtin (any arg count is spec-valid,
/// so `string_only_method_arity_ok` always returns `true`), so without this a
/// `this.charAt(0)` on a class instance is lowered to `String.prototype.charAt`
/// with the receiver coerced to `"[object Object]"` (yielding `"["`, `"o"`, …).
/// The `yaml` package's `Lexer.charAt(n)` is exactly this shape — the tokenizer
/// then reads garbage, and its `*lex` state machine never advances `pos`,
/// spinning forever. A genuine string receiver is unaffected: it has no known
/// class, so this returns `false` and the string path (or the runtime
/// `jsval.is_string()` arm of `js_native_call_method`) still applies.
pub(crate) fn receiver_class_defines_method(ctx: &FnCtx<'_>, object: &Expr, name: &str) -> bool {
    let Some(mut class_name) = receiver_class_name(ctx, object) else {
        return false;
    };
    // Bounded walk up the inheritance chain (defensive against a cyclic
    // `extends_name` in malformed input).
    for _ in 0..64 {
        let Some(class) = ctx.classes.get(&class_name) else {
            return false;
        };
        if class.methods.iter().any(|m| m.name == name)
            || class.getters.iter().any(|(g, _)| g == name)
            // An instance FIELD of that name shadows the builtin too: its
            // init can be a function value (`charAt = (n) => …`), and even a
            // non-function field makes `obj.charAt(0)` a runtime "not a
            // function" TypeError — never the String builtin. A computed key
            // (`key_expr`) could evaluate to `name`, so treat it as defining
            // the member (same conservatism as `class_chain_has_field_named`).
            || class
                .fields
                .iter()
                .any(|f| f.key_expr.is_some() || (!f.is_private && f.name == name))
        {
            return true;
        }
        match &class.extends_name {
            Some(parent) => class_name = parent.clone(),
            None => return false,
        }
    }
    false
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
        Expr::PropertyGet {
            object, property, ..
        } if matches!(object.as_ref(), Expr::ExternFuncRef { .. })
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
