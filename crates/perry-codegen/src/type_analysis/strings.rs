//! String / Set / Map / URLSearchParams static-type predicates.
//!
//! Split out of `type_analysis.rs` (file-size gate). Pure code move.

use super::*;

use perry_hir::{BinaryOp, Expr, UnaryOp};
use perry_types::Type as HirType;

use crate::expr::FnCtx;
use crate::type_analysis_class_fields::{
    class_field_declared_type, class_field_global_index, declared_field_type,
};
use crate::type_analysis_facts::{
    function_type_from_decl, hir_inferred_refinable_type, hir_inferred_static_type,
};
use crate::type_analysis_net::{net_result_class, net_result_type};

pub(crate) fn is_set_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::SetNew | Expr::SetNewFromArray(_) => true,
        Expr::LocalGet(id) => matches!(
            ctx.local_types.get(id),
            Some(HirType::Generic { base, .. }) if base == "Set"
        ),
        // `this.field` where the field is declared as `Set<T>` on the
        // enclosing class. Same rationale as is_map_expr.
        Expr::PropertyGet { object, property } => {
            if let Some(cls_name) = receiver_class_name(ctx, object) {
                if let Some(cls) = ctx.classes.get(&cls_name) {
                    if let Some(field) = cls.fields.iter().find(|f| f.name == *property) {
                        return matches!(
                            field.ty,
                            HirType::Generic { ref base, .. } if base == "Set"
                        );
                    }
                }
            }
            false
        }
        _ => false,
    }
}

pub(crate) fn set_static_type_args<'a>(ctx: &'a FnCtx<'_>, e: &Expr) -> Option<&'a [HirType]> {
    match e {
        Expr::LocalGet(id) => match ctx.local_types.get(id) {
            Some(HirType::Generic { base, type_args }) if base == "Set" => {
                Some(type_args.as_slice())
            }
            _ => None,
        },
        Expr::PropertyGet { object, property } => {
            let cls_name = receiver_class_name(ctx, object)?;
            let cls = ctx.classes.get(&cls_name)?;
            let field = cls.fields.iter().find(|f| f.name == *property)?;
            match &field.ty {
                HirType::Generic { base, type_args } if base == "Set" => Some(type_args.as_slice()),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Issue #650: detect URLSearchParams receivers for `sp.size` property
/// access. URLSearchParams is allocated as a generic ObjectHeader; the
/// type system tracks it as `HirType::Named("URLSearchParams")`. Used by
/// the codegen `Expr::PropertyGet { property: "size" }` arm to route
/// through `js_url_search_params_size` instead of returning undefined.
pub(crate) fn is_url_search_params_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::UrlSearchParamsNew(_) => true,
        Expr::LocalGet(id) => matches!(
            ctx.local_types.get(id),
            Some(HirType::Named(name)) if name == "URLSearchParams"
        ),
        Expr::UrlGetSearchParams(_) => true,
        // `urlInstance.searchParams` — the HIR keeps this as a generic
        // PropertyGet (the URL HIR variant only fires for direct typed
        // receivers in `lower_member`). Detect the chained access here
        // so `url.searchParams.size` works without an intermediate let.
        Expr::PropertyGet { object, property } if property == "searchParams" => {
            if let Expr::LocalGet(id) = object.as_ref() {
                return matches!(
                    ctx.local_types.get(id),
                    Some(HirType::Named(name)) if name == "URL"
                );
            }
            matches!(object.as_ref(), Expr::UrlNew { .. })
        }
        _ => false,
    }
}

pub(crate) fn is_map_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::MapNew | Expr::MapNewFromArray(_) => true,
        Expr::LocalGet(id) => matches!(
            ctx.local_types.get(id),
            Some(HirType::Generic { base, .. }) if base == "Map"
        ),
        // `this.field` where the field is declared as `Map<K, V>` on
        // the enclosing class. Needed so `this.handlers.set(...)` /
        // `this.handlers.get(...)` inside class methods dispatch
        // through the Map fast path instead of the dynamic field-set
        // fallback.
        Expr::PropertyGet { object, property } => {
            if let Some(cls_name) = receiver_class_name(ctx, object) {
                if let Some(cls) = ctx.classes.get(&cls_name) {
                    if let Some(field) = cls.fields.iter().find(|f| f.name == *property) {
                        return matches!(
                            field.ty,
                            HirType::Generic { ref base, .. } if base == "Map"
                        );
                    }
                }
            }
            false
        }
        _ => false,
    }
}

pub(crate) fn map_static_type_args<'a>(ctx: &'a FnCtx<'_>, e: &Expr) -> Option<&'a [HirType]> {
    match e {
        Expr::LocalGet(id) => match ctx.local_types.get(id) {
            Some(HirType::Generic { base, type_args }) if base == "Map" => {
                Some(type_args.as_slice())
            }
            _ => None,
        },
        Expr::PropertyGet { object, property } => {
            let cls_name = receiver_class_name(ctx, object)?;
            let cls = ctx.classes.get(&cls_name)?;
            let field = cls.fields.iter().find(|f| f.name == *property)?;
            match &field.ty {
                HirType::Generic { base, type_args } if base == "Map" => Some(type_args.as_slice()),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Stricter variant of `is_string_expr` that requires the type to be
/// definitely `String` — unions are NOT treated as strings. Used in the
/// string-concat fast path where dispatching through the string-only
/// codegen on a non-string union value produces garbage (e.g. masking an
/// f64 number's bits with POINTER_MASK yields a null pointer).
///
/// For JS `+` semantics on a union of string and number, the correct
/// behavior depends on the runtime value: `1 + "foo"` concatenates,
/// `1 + 42` adds. The generic numeric-add path (with `js_number_coerce`
/// fallback) handles narrowed-numeric cases correctly and is safer than
/// the string path when the value might actually be a number.
pub(crate) fn is_definitely_string_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::String(_) | Expr::WtfString(_) => true,
        Expr::LocalGet(id) => matches!(
            ctx.local_types.get(id),
            Some(HirType::String | HirType::StringLiteral(_))
        ),
        Expr::PathToNamespacedPath(path) => is_definitely_string_expr(ctx, path),
        Expr::PathWin32 {
            method: perry_hir::PathWin32Method::ToNamespacedPath,
            args,
        } => args
            .first()
            .is_some_and(|arg| is_definitely_string_expr(ctx, arg)),
        Expr::StringCoerce(_)
        | Expr::TypeOf(_)
        | Expr::ArrayJoin { .. }
        | Expr::JsonStringify(_)
        | Expr::JsonStringifyPretty { .. }
        | Expr::JsonStringifyFull(..)
        | Expr::StringFromCodePoint(_)
        | Expr::StringFromCharCode(_)
        | Expr::StringFromCharCodeSpread(_)
        | Expr::StringRaw { .. }
        | Expr::FsReadFileSync(_)
        | Expr::FsReadFileBinary(_)
        | Expr::PathSep
        | Expr::PathDelimiter
        | Expr::PathJoin(..)
        | Expr::PathDirname(_)
        | Expr::PathBasename(_)
        | Expr::PathExtname(_)
        | Expr::PathResolve(_)
        | Expr::PathNormalize(_)
        | Expr::PathResolveJoin(..)
        | Expr::PathWin32Join(..)
        | Expr::PathWin32 {
            method:
                perry_hir::PathWin32Method::Dirname
                | perry_hir::PathWin32Method::Basename
                | perry_hir::PathWin32Method::BasenameExt
                | perry_hir::PathWin32Method::Extname
                | perry_hir::PathWin32Method::Normalize
                | perry_hir::PathWin32Method::Format
                | perry_hir::PathWin32Method::Relative
                | perry_hir::PathWin32Method::Resolve
                | perry_hir::PathWin32Method::ResolveJoin,
            ..
        }
        | Expr::ProcessVersion
        | Expr::ProcessCwd
        | Expr::ProcessTitle
        | Expr::OsArch
        | Expr::OsType
        | Expr::OsPlatform
        | Expr::OsRelease
        | Expr::OsHostname
        | Expr::OsEOL
        | Expr::OsDevNull
        | Expr::OsEndianness
        | Expr::OsMachine
        | Expr::OsVersion => true,
        // `.toString()` always returns a string regardless of receiver
        // type, so it's safe to count as definitely-string for concat.
        // Same for other unary string-returning string methods.
        Expr::Call { callee, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { property, .. } if matches!(
                    property.as_str(),
                    "toString" | "toLowerCase" | "toUpperCase" | "trim"
                        | "trimStart" | "trimEnd" | "slice" | "substring"
                        | "substr" | "charAt" | "repeat" | "replace"
                        | "replaceAll" | "padStart" | "padEnd" | "concat"
                        | "normalize" | "toFixed" | "toPrecision" | "toExponential"
                )
            ) =>
        {
            true
        }
        Expr::Binary {
            op: BinaryOp::Add,
            left,
            right,
        } => is_definitely_string_expr(ctx, left) || is_definitely_string_expr(ctx, right),
        // Ternary `cond ? a : b` is definitely a string when BOTH
        // branches are definitely strings. Without this, code like
        //   (d ? "D" : "") + (v ? "V" : "")
        // misses the string-concat fast path because each ternary is
        // typed as Any, the `+` falls through to numeric Add, both
        // operands get js_number_coerce'd (string → NaN), and the
        // result prints as "NaN" instead of the concatenation.
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => is_definitely_string_expr(ctx, then_expr) && is_definitely_string_expr(ctx, else_expr),
        Expr::PropertyGet { object, property }
            if is_process_namespace_version_property(object, property) =>
        {
            true
        }
        _ => false,
    }
}

/// Resolve the declared type of `<object>.<field>` when `object` is a
/// known user class or interface that declares (or inherits) a field
/// named `field`. Returns `None` when the receiver isn't a tracked
/// class/interface, or when no such field is declared on it.
///
/// Used to keep name-only field heuristics (the Error `.message` /
/// `.stack` / `.name` string assumption) from hijacking a user class
/// whose own field happens to share that name with a non-string type
/// (e.g. `effect`'s `RedBlackTreeIterator.stack: Array<...>` — #321).
pub(crate) fn is_string_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::String(_) | Expr::WtfString(_) => true,
        Expr::LocalGet(id) => {
            match ctx.local_types.get(id) {
                Some(HirType::String | HirType::StringLiteral(_)) => true,
                // Union(String, Null/Void) — nullable strings are still
                // strings at runtime when non-null. The ?. and != null
                // guard paths lower the non-null case through the string
                // method dispatch. Without this, `(s: string | null).
                // toUpperCase()` fell through to the generic path and
                // returned undefined.
                Some(HirType::Union(members)) => {
                    members
                        .iter()
                        .any(|m| matches!(m, HirType::String | HirType::StringLiteral(_)))
                }
                _ => false,
            }
        }
        // arr[i] where arr is Array<string> → element is a string.
        // Lets `this.parts[i].length` use the string fast path inline
        // without needing an intermediate let binding. Also str[i] on
        // a string-typed receiver returns a single-character string,
        // so the tokenizer pattern `input[pos] >= "0"` routes through
        // string comparison.
        Expr::IndexGet { object, .. } => {
            match static_type_of(ctx, object) {
                Some(HirType::Array(elem)) if matches!(*elem, HirType::String) => true,
                Some(HirType::String) => true,
                _ => false,
            }
        }
        // Enum string members lower to string literals at the use
        // site, so a comparison like `c === Color.Red` should fire
        // the string equality fast path.
        Expr::EnumMember { enum_name, member_name } => {
            matches!(
                ctx.enums.get(&(enum_name.clone(), member_name.clone())),
                Some(perry_hir::EnumValue::String(_))
            )
        }
        Expr::Binary { op: BinaryOp::Add, left, right } => {
            is_string_expr(ctx, left) || is_string_expr(ctx, right)
        }
        Expr::PathToNamespacedPath(path) => is_definitely_string_expr(ctx, path),
        Expr::PathWin32 {
            method: perry_hir::PathWin32Method::ToNamespacedPath,
            args,
        } => args
            .first()
            .is_some_and(|arg| is_definitely_string_expr(ctx, arg)),
        // String coerce, JSON.stringify, ArrayJoin, etc. all return
        // strings.
        Expr::StringCoerce(_)
        | Expr::TypeOf(_)
        | Expr::ArrayJoin { .. }
        | Expr::JsonStringifyFull(..)
        | Expr::FsReadFileSync(_)
        | Expr::FsReadFileBinary(_)
        | Expr::PathJoin(..)
        | Expr::PathDirname(_)
        | Expr::PathBasename(_)
        | Expr::PathExtname(_)
        | Expr::PathResolve(_)
        | Expr::PathNormalize(_)
        | Expr::PathResolveJoin(..)
        | Expr::PathWin32Join(..)
        | Expr::PathWin32 {
            method:
                perry_hir::PathWin32Method::Dirname
                | perry_hir::PathWin32Method::Basename
                | perry_hir::PathWin32Method::BasenameExt
                | perry_hir::PathWin32Method::Extname
                | perry_hir::PathWin32Method::Normalize
                | perry_hir::PathWin32Method::Format
                | perry_hir::PathWin32Method::Relative
                | perry_hir::PathWin32Method::Resolve
                | perry_hir::PathWin32Method::ResolveJoin,
            ..
        } => true,
        // String.fromCodePoint(...) / String.fromCharCode(...) / str.at(i)
        // / RegExp.source|flags — all produce string handles.
        Expr::StringFromCodePoint(_)
        | Expr::StringFromCharCode(_)
        | Expr::StringFromCharCodeSpread(_)
        | Expr::StringRaw { .. }
        | Expr::StringAt { .. }
        | Expr::RegExpSource(_)
        | Expr::RegExpFlags(_)
        // Date.prototype.to*String() → string
        | Expr::DateToString(_)
        | Expr::DateToDateString(_)
        | Expr::DateToTimeString(_)
        | Expr::DateToUTCString(_)
        | Expr::DateToLocaleString(_)
        | Expr::DateToLocaleDateString(_)
        | Expr::DateToLocaleTimeString(_)
        | Expr::DateToISOString(_)
        | Expr::DateToJSON(_)
        // node:path constants
        | Expr::PathSep
        | Expr::PathDelimiter
        // JSON.stringify returns a string. #853: `JsonStringifyFull(..)`
        // is already enumerated in the earlier (line ~878) arm — listing
        // it again here was dead.
        | Expr::JsonStringify(_)
        | Expr::JsonStringifyPretty { .. } => true,
        // process.* / os.* string-returning accessors. These lower to runtime
        // calls that return raw StringHeader* pointers, NaN-boxed with STRING_TAG
        // in expr.rs. Without this, `process.version.startsWith('v')` falls
        // through to the generic native method dispatch and returns undefined.
        Expr::ProcessVersion
        | Expr::ProcessCwd
        | Expr::ProcessTitle
        | Expr::OsArch
        | Expr::OsType
        | Expr::OsPlatform
        | Expr::OsRelease
        | Expr::OsHostname
        | Expr::OsEOL
        | Expr::OsDevNull
        | Expr::OsEndianness
        | Expr::OsMachine
        | Expr::OsVersion => true,
        // `obj.toString()` always returns a string. Same for the
        // string-returning method family (trim, trimStart, trimEnd,
        // toLowerCase, toUpperCase, slice, substring, charAt, repeat,
        // replace, replaceAll, split's first elem, etc. — limited to
        // unary methods on a string receiver). Recognize these so
        // chained calls like `s.trimStart().trimEnd()` detect the
        // inner result as a string.
        Expr::Call { callee, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { property, object } if matches!(
                    property.as_str(),
                    "toString" | "toLowerCase" | "toUpperCase" | "trim"
                        | "trimStart" | "trimEnd" | "slice" | "substring"
                        | "substr" | "charAt" | "repeat" | "replace"
                        | "replaceAll" | "padStart" | "padEnd" | "concat"
                        | "normalize" | "at" | "toWellFormed"
                ) && (
                    is_string_expr(ctx, object)
                        || matches!(property.as_str(), "toString")
                )
            ) =>
        {
            true
        }
        // Error instance field access — e.message / e.stack / e.name
        // all route through the runtime's GC_TYPE_ERROR dispatch and
        // return string pointers. Recognize them so chained calls like
        // `e.stack!.includes("...")` hit the string method fast path.
        //
        // BUT this name-only heuristic must NOT hijack a user class /
        // interface whose own field happens to be called `stack` /
        // `name` / `message` with a non-string declared type. The
        // RedBlackTreeIterator in `effect` has `readonly stack:
        // Array<Node<K,V>>`; without this guard `this.stack[i]` was
        // mis-lowered as a string `char_at` (garbage element reads →
        // null SortedSet iteration, #321). When the receiver resolves
        // to a concrete declared field type, defer to it; only fall
        // back to the Error-string assumption when the receiver's type
        // is genuinely unknown (a real caught `Error`/`unknown`/`any`).
        Expr::PropertyGet { object, property }
            // `.stack` excluded — may be an array via `Error.prepareStackTrace`.
            if matches!(property.as_str(), "message" | "name") =>
        {
            // If the receiver is a known user class / interface that
            // *declares* a field with this name, that field's declared
            // type wins over the name-only Error heuristic.
            if let Some(declared) = declared_field_type(ctx, object, property) {
                return matches!(declared, HirType::String);
            }
            // Otherwise it's an Error-shaped property (caught `e`,
            // `unknown`/`any`, or an untracked receiver) → string.
            true
        }
        // Namespace `node:process` exports share the same runtime process
        // surface as bare `process`. Keep the string method dispatch
        // available for namespace imports:
        // `import * as process from "node:process"; process.version.startsWith("v")`.
        Expr::PropertyGet { object, property }
            if is_process_namespace_version_property(object, property) =>
        {
            true
        }
        // Perry's native crypto.generateKeyPairSync returns a plain object
        // with PEM string fields. Refining these fields keeps
        // `pair.publicKey.includes(...)` on the string fast path.
        Expr::PropertyGet { object, property }
            if matches!(property.as_str(), "publicKey" | "privateKey")
                && matches!(
                    static_type_of(ctx, object),
                    Some(HirType::Named(ref name)) if name == "CryptoKeyPair"
                ) =>
        {
            true
        }
        // PropertyGet on a known class field with declared type String.
        Expr::PropertyGet { object, property } => {
            let Some(class_name) = receiver_class_name(ctx, object) else {
                return false;
            };
            let Some(class) = ctx.classes.get(&class_name) else {
                return false;
            };
            class
                .fields
                .iter()
                .find(|f| f.name == *property)
                .map(|f| matches!(f.ty, HirType::String))
                .unwrap_or(false)
        }
        // `crypto.createHash(alg).update(data).digest(enc)` chain — only
        // when an encoding is given. Recognized so chained `.length` /
        // `.includes` / `===` on the resulting hex/base64 string hit the
        // string fast paths. The no-arg `digest()` returns a Buffer, not a
        // string, so it must NOT be classified here — otherwise
        // `digest().toString('hex')` skips the buffer encoding path and
        // mis-reads the bytes as Latin-1 (#1353).
        Expr::Call { callee, args, .. }
            if is_crypto_digest_chain(callee)
                && matches!(args.first(), Some(a) if !matches!(a, Expr::Undefined)) =>
        {
            true
        }
        // atob/btoa always return strings.
        Expr::Atob(_) | Expr::Btoa(_) => true,
        _ => false,
    }
}
