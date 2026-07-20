//! Array-method folding and `typeof <X>.<method>` recognisers.
//!
//! Extracted from `lower/mod.rs`. `try_fold_array_method_call` reduces
//! synthetic `Expr::Call` shapes built by the optional-chain lowering
//! into dedicated `Expr::Array<Method>` HIR variants. The
//! `is_known_*` helpers feed the AST-level `typeof Object.<method>` /
//! `typeof Array.<method>` / `typeof "".<method>` folds so feature
//! detection (`typeof X.foo === "function"`) sees Perry's actual
//! built-ins. All helpers were `pub(crate)` already; visibility
//! preserved.

use crate::ir::*;

/// Try to fold an `Expr::Call { callee: PropertyGet { object, property }, args }`
/// into an `Expr::Array<Method>` HIR variant for known array methods. Used by
/// the optional-chain Call lowering, which constructs `Expr::Call` directly
/// (bypassing the regular `lower_expr` array fast-path detection that would
/// otherwise catch `obj.map(cb)` etc. on an AST `MemberExpr` callee).
///
/// Returns `Some(rewritten_expr)` when the callee is a PropertyGet on a known
/// array method name and the arity matches; returns `None` otherwise so the
/// caller can fall back to the generic `Expr::Call` form.
pub(crate) fn try_fold_array_method_call(call: Expr) -> Expr {
    let (callee, args) = match call {
        Expr::Call { callee, args, .. } => (callee, args),
        other => return other,
    };
    let (object, property) = match *callee {
        Expr::PropertyGet {
            object, property, ..
        } => (object, property),
        other => {
            return Expr::Call {
                callee: Box::new(other),
                args,
                type_args: Vec::new(),
                byte_offset: 0,
            };
        }
    };
    // Helper to rebuild the original Call if we don't want to fold.
    let rebuild = |obj: Box<Expr>, prop: String, args: Vec<Expr>| Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            byte_offset: 0,
            object: obj,
            property: prop,
        }),
        args,
        type_args: Vec::new(),
        byte_offset: 0,
    };
    match property.as_str() {
        "map" if !args.is_empty() => {
            let cb = args.into_iter().next().unwrap();
            Expr::ArrayMap {
                array: object,
                callback: Box::new(cb),
            }
        }
        "filter" if !args.is_empty() => {
            let cb = args.into_iter().next().unwrap();
            Expr::ArrayFilter {
                array: object,
                callback: Box::new(cb),
            }
        }
        "forEach" if !args.is_empty() => {
            let cb = args.into_iter().next().unwrap();
            Expr::ArrayForEach {
                array: object,
                callback: Box::new(cb),
            }
        }
        "find" if !args.is_empty() => {
            let cb = args.into_iter().next().unwrap();
            Expr::ArrayFind {
                array: object,
                callback: Box::new(cb),
            }
        }
        "findIndex" if !args.is_empty() => {
            let cb = args.into_iter().next().unwrap();
            Expr::ArrayFindIndex {
                array: object,
                callback: Box::new(cb),
            }
        }
        "findLast" if !args.is_empty() => {
            let cb = args.into_iter().next().unwrap();
            Expr::ArrayFindLast {
                array: object,
                callback: Box::new(cb),
            }
        }
        "findLastIndex" if !args.is_empty() => {
            let cb = args.into_iter().next().unwrap();
            Expr::ArrayFindLastIndex {
                array: object,
                callback: Box::new(cb),
            }
        }
        "some" if !args.is_empty() => {
            let cb = args.into_iter().next().unwrap();
            Expr::ArraySome {
                array: object,
                callback: Box::new(cb),
            }
        }
        "every" if !args.is_empty() => {
            let cb = args.into_iter().next().unwrap();
            Expr::ArrayEvery {
                array: object,
                callback: Box::new(cb),
            }
        }
        _ => rebuild(object, property, args),
    }
}

/// Names of well-known `Object.<name>` static methods. Used by the typeof
/// fast path so `typeof Object.groupBy === "function"` evaluates to true
/// at compile time.
pub(crate) fn is_known_object_static_method(name: &str) -> bool {
    matches!(
        name,
        "keys"
            | "values"
            | "entries"
            | "fromEntries"
            | "assign"
            | "is"
            | "hasOwn"
            | "freeze"
            | "seal"
            | "preventExtensions"
            | "create"
            | "isFrozen"
            | "isSealed"
            | "isExtensible"
            | "getPrototypeOf"
            | "setPrototypeOf"
            | "defineProperty"
            | "defineProperties"
            | "getOwnPropertyDescriptor"
            | "getOwnPropertyDescriptors"
            | "getOwnPropertyNames"
            | "getOwnPropertySymbols"
            | "groupBy"
    )
}

/// Names of well-known `Array.<name>` static methods.
pub(crate) fn is_known_array_static_method(name: &str) -> bool {
    matches!(name, "isArray" | "from" | "of" | "fromAsync")
}

/// Names of `String.prototype.<name>` instance methods that Perry's
/// runtime implements (or short-circuits) — used by the `typeof
/// "".methodName` AST fold so feature-detection checks like
/// `if (typeof "".isWellFormed === "function")` see the methods that
/// the runtime would actually dispatch successfully.
pub(crate) fn is_known_string_prototype_method(name: &str) -> bool {
    matches!(
        name,
        // ES2015+ classics
        "charAt" | "charCodeAt" | "codePointAt" | "concat" | "endsWith"
        | "includes" | "indexOf" | "lastIndexOf" | "match" | "matchAll"
        | "normalize" | "padEnd" | "padStart" | "repeat" | "replace"
        | "replaceAll" | "search" | "slice" | "split" | "startsWith"
        | "substring" | "toLowerCase" | "toUpperCase" | "toLocaleLowerCase"
        | "toLocaleUpperCase" | "trim" | "trimEnd" | "trimStart" | "at"
        // ES2024
        | "isWellFormed" | "toWellFormed"
    )
}

/// Names of the universal `Object.prototype.<name>` methods inherited by
/// every object and boxed primitive (numbers, strings, booleans). Used by
/// the `typeof <Ctor>.prototype.<m>` AST fold (#2058) so feature-detection
/// idioms like `typeof Object.prototype.isPrototypeOf === "function"` and
/// `typeof Number.prototype.hasOwnProperty === "function"` agree with Node.
/// These are real functions on `Object.prototype`, so they resolve to
/// callable values on any inheriting receiver.
pub(crate) fn is_known_object_prototype_method(name: &str) -> bool {
    matches!(
        name,
        "hasOwnProperty"
            | "isPrototypeOf"
            | "propertyIsEnumerable"
            | "toLocaleString"
            | "toString"
            | "valueOf"
            | "constructor"
            // Annex B §B.2.2 legacy accessor helpers.
            | "__defineGetter__"
            | "__defineSetter__"
            | "__lookupGetter__"
            | "__lookupSetter__"
    )
}

/// Names of `Array.prototype.<name>` instance methods that Perry's runtime
/// implements (or short-circuits) — used by the `typeof Array.prototype.<m>`
/// / `typeof [].<m>` AST fold (#1777) so feature detection and the indirect
/// prototype-borrow idiom (`[].slice.call(args)`) see callable values.
pub(crate) fn is_known_array_prototype_method(name: &str) -> bool {
    matches!(
        name,
        // mutators
        "push" | "pop" | "shift" | "unshift" | "splice" | "sort" | "reverse"
        | "fill" | "copyWithin"
        // accessors / iteration
        | "concat" | "slice" | "join" | "indexOf" | "lastIndexOf" | "includes"
        | "find" | "findIndex" | "findLast" | "findLastIndex" | "at"
        | "forEach" | "map" | "filter" | "reduce" | "reduceRight" | "some"
        | "every" | "flat" | "flatMap" | "keys" | "values" | "entries"
        | "toString" | "toLocaleString" | "with" | "toReversed" | "toSorted"
        | "toSpliced"
    )
}

/// Names of `Promise.<name>` static methods recognised by Perry's codegen
/// (`crates/perry-codegen/src/lower_call/console_promise.rs`).
pub(crate) fn is_known_promise_static_method(name: &str) -> bool {
    matches!(
        name,
        "resolve" | "reject" | "all" | "race" | "allSettled" | "any" | "withResolvers" | "try"
    )
}

/// Names of `Math.<name>` static functions Perry's runtime implements.
pub(crate) fn is_known_math_static_method(name: &str) -> bool {
    matches!(
        name,
        "abs"
            | "acos"
            | "acosh"
            | "asin"
            | "asinh"
            | "atan"
            | "atan2"
            | "atanh"
            | "cbrt"
            | "ceil"
            | "clz32"
            | "cos"
            | "cosh"
            | "exp"
            | "expm1"
            | "floor"
            | "fround"
            | "f16round"
            | "hypot"
            | "imul"
            | "log"
            | "log10"
            | "log1p"
            | "log2"
            | "max"
            | "min"
            | "pow"
            | "random"
            | "round"
            | "sign"
            | "sin"
            | "sinh"
            | "sqrt"
            | "tan"
            | "tanh"
            | "trunc"
    )
}

/// Names of `JSON.<name>` static functions Perry's runtime implements.
pub(crate) fn is_known_json_static_method(name: &str) -> bool {
    matches!(name, "parse" | "stringify" | "rawJSON" | "isRawJSON")
}

/// Names of `Atomics.<name>` static functions Perry's runtime implements.
pub(crate) fn is_known_atomics_static_method(name: &str) -> bool {
    matches!(
        name,
        "load"
            | "isLockFree"
            | "store"
            | "add"
            | "sub"
            | "and"
            | "or"
            | "xor"
            | "exchange"
            | "compareExchange"
            | "notify"
            | "wait"
            | "waitAsync"
    )
}

/// Names of `Number.<name>` static functions Perry's runtime implements.
pub(crate) fn is_known_number_static_method(name: &str) -> bool {
    matches!(
        name,
        "isFinite" | "isInteger" | "isNaN" | "isSafeInteger" | "parseFloat" | "parseInt"
    )
}

/// Names of `String.<name>` static functions Perry's runtime implements.
pub(crate) fn is_known_string_static_method(name: &str) -> bool {
    matches!(name, "fromCharCode" | "fromCodePoint" | "raw")
}

/// True when `<obj_name>.<prop_name>` names a built-in function value
/// (a static method on a known namespace). Used by the `typeof
/// <NS>.<static>` and `typeof <NS>.<static>.<bind|call|apply>` AST folds
/// (#2143). Direct calls to these statics (`Promise.resolve(x)`,
/// `Math.min(1,2)`) are special-cased in codegen; this fold makes the
/// value-read agree with Node's "function" typeof so feature-detection
/// idioms (and Test262's `propertyHelper.js` family) see callable
/// methods.
pub(crate) fn is_known_namespace_static_function(obj_name: &str, prop_name: &str) -> bool {
    match obj_name {
        "Object" => is_known_object_static_method(prop_name),
        "Array" => is_known_array_static_method(prop_name),
        "Promise" => is_known_promise_static_method(prop_name),
        "Math" => is_known_math_static_method(prop_name),
        "JSON" => is_known_json_static_method(prop_name),
        "Atomics" => is_known_atomics_static_method(prop_name),
        "Number" => is_known_number_static_method(prop_name),
        "String" => is_known_string_static_method(prop_name),
        // #2877: `ArrayBuffer.isView` is a real static function (folded to the
        // `util.types.isArrayBufferView` predicate at call sites). The bare
        // value-read must report typeof "function".
        "ArrayBuffer" => prop_name == "isView",
        // #2874: `Iterator.from` is a real static function in Node 22+.
        "Iterator" => prop_name == "from",
        "Response" => matches!(prop_name, "json" | "redirect" | "error"),
        _ => false,
    }
}
