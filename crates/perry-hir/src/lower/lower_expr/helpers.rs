//! Small helper functions for `lower_expr` and friends, extracted from the
//! trunk `lower_expr.rs` so the entry-point file stays under the 2,000-LOC
//! soft cap. Pure code move — no behavior change.

use super::*;
// Pull in the parent `lower` module's full (re-exported) surface so moved
// helpers resolve names like `LoweringContext`, `expr_member`,
// `is_builtin_function`, etc. exactly as they did in the trunk.
use crate::lower::*;
use anyhow::{anyhow, Result};
use perry_types::{LocalId, Type};
use swc_common::Spanned;
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower_types::extract_ts_type_with_ctx;

/// Whether `PERRY_GLOBAL_SCRIPT_THIS` is set — compile the program as a
/// *global script* rather than a CJS module, so module top-level `this`
/// lowers to `globalThis` instead of the `module.exports` stand-in
/// (`Expr::ModuleTopThis`). This matches a conforming Test262 host (and the
/// Node oracle's `vm.runInThisContext`, #5346/#5511); the default stays
/// CJS so standalone builds match `node --experimental-strip-types`. Read
/// once per process — the env is fixed for the lifetime of a compile (#5579).
pub(crate) fn global_script_this_enabled() -> bool {
    use std::sync::OnceLock;
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| match std::env::var("PERRY_GLOBAL_SCRIPT_THIS") {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            !matches!(v.as_str(), "" | "0" | "off" | "false" | "no")
        }
        Err(_) => false,
    })
}

pub(crate) fn throw_reference_error_expr(helper_name: &str) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::ExternFuncRef {
            name: helper_name.to_string(),
            param_types: Vec::new(),
            return_type: Type::Any,
        }),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
    }
}

/// #5989: lower a strict-mode assignment to an identifier with no lexical
/// binding. Per spec (PutValue on a reference that resolves to the global
/// environment), an EXISTING global property is a normal property write —
/// Next.js 16's `cacheComponents` node-environment extensions reassign
/// `Date` exactly this way in strict CJS (`Date = createDate(Date)`), and
/// the old unconditional throw made the install fail at boot, disarming
/// dynamic-IO clock detection. Only a genuinely absent binding throws; the
/// runtime helper does the presence probe + write-back (mirroring the
/// `js_global_update` shape for `++x` on globals). Argument order preserves
/// spec evaluation order: the RHS evaluates before the reference check can
/// throw. Sloppy mode never reaches here (it lowers to a globalThis property
/// set that may CREATE the binding).
///
/// Shared by both assignment-lowering arms (`expr_assign.rs`'s
/// `lower_assignment_target` and `lower_expr/assignment.rs`) so they can't
/// drift.
pub(crate) fn strict_global_assign_existing_or_throw(name: String, value: Box<Expr>) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::ExternFuncRef {
            name: "js_global_assign_existing_or_throw".to_string(),
            param_types: vec![Type::Any, Type::Any],
            return_type: Type::Any,
        }),
        args: vec![Expr::String(name), *value],
        type_args: vec![],
        byte_offset: 0,
    }
}

pub(crate) fn is_known_global_identifier_name(name: &str) -> bool {
    matches!(
        name,
        "console"
            | "process"
            | "globalThis"
            | "Buffer"
            | "Date"
            | "Intl"
            | "JSON"
            | "Math"
            | "Object"
            | "Array"
            | "String"
            | "Number"
            | "Boolean"
            | "Function"
            | "Error"
            | "TypeError"
            | "RangeError"
            | "SyntaxError"
            | "ReferenceError"
            | "EvalError"
            | "URIError"
            | "AggregateError"
            | "Promise"
            | "Map"
            | "Set"
            | "RegExp"
            | "Symbol"
            | "WeakMap"
            | "WeakSet"
            | "WeakRef"
            | "FinalizationRegistry"
            | "DisposableStack"
            | "AsyncDisposableStack"
            | "SuppressedError"
            | "Proxy"
            | "Reflect"
            | "Uint8Array"
            | "Int8Array"
            | "Int16Array"
            | "Uint16Array"
            | "Int32Array"
            | "Uint32Array"
            | "Float16Array"
            | "Float32Array"
            | "Float64Array"
            | "TextEncoder"
            | "TextDecoder"
            | "URL"
            | "URLSearchParams"
            | "AbortController"
            | "Blob"
            | "FormData"
            | "File"
            | "Headers"
            | "Request"
            | "Response"
            | "fetch"
            | "crypto"
            | "performance"
            | "queueMicrotask"
            | "structuredClone"
            | "atob"
            | "btoa"
            | "BigInt"
            | "WebAssembly"
            // TC39 Temporal namespace (#4686) — a bare `Temporal` resolves to
            // `globalThis.Temporal`.
            | "Temporal"
    ) || is_builtin_global_value_name(name)
}

pub(crate) fn is_fetch_global_value_name(name: &str) -> bool {
    matches!(
        name,
        "fetch" | "Blob" | "File" | "FormData" | "Headers" | "Request" | "Response"
    )
}

pub(crate) fn is_cjs_style_native_default_import(module_name: &str) -> bool {
    matches!(
        module_name,
        "async_hooks"
            | "child_process"
            | "cluster"
            | "constants"
            | "dns"
            | "dns/promises"
            | "events"
            | "module"
            | "os"
            | "path"
            | "path/posix"
            | "path/win32"
            | "punycode"
            | "querystring"
            | "sys"
            | "url"
            | "util"
    )
}

pub(crate) fn wrap_with_gets(property: &str, fallback: Expr, envs: Vec<LocalId>) -> Expr {
    envs.into_iter()
        .rev()
        .fold(fallback, |fallback, env_id| Expr::WithGet {
            object: Box::new(Expr::LocalGet(env_id)),
            property: property.to_string(),
            fallback: Box::new(fallback),
        })
}

/// The HOLE-sentinel `Stmt::Let` for a with-fallback implicit global,
/// emitted just ahead of the with statement that minted it.
pub(crate) fn with_implicit_unset_let(id: LocalId, name: String) -> Stmt {
    Stmt::Let {
        id,
        name,
        ty: Type::Any,
        mutable: true,
        init: Some(Expr::Call {
            callee: Box::new(Expr::ExternFuncRef {
                name: "js_with_implicit_unset".to_string(),
                param_types: vec![],
                return_type: Type::Any,
            }),
            args: vec![],
            type_args: vec![],
            byte_offset: 0,
        }),
    }
}

pub(crate) fn with_set_fallback_for_ident(
    ctx: &mut LoweringContext,
    name: &str,
) -> WithSetFallback {
    if let Some(id) = ctx.lookup_local(name) {
        if ctx.is_local_immutable(id) {
            WithSetFallback::ThrowConstAssignment
        } else {
            WithSetFallback::Local(id)
        }
    } else if ctx.lookup_class(name).is_some() || ctx.lookup_func(name).is_some() {
        WithSetFallback::Ignore
    } else if ctx.current_strict {
        WithSetFallback::ThrowReferenceError
    } else {
        eprintln!(
            "  Warning: Assignment to undeclared variable '{}', creating implicit local",
            name
        );
        // Sloppy implicit global — must survive the with-body block scope so
        // reads AFTER the with statement resolve to the same binding
        // (`with (o) { result = f(); } … use result` — test262 S13.2.2_A19).
        // Whether the binding materialises is decided at RUNTIME (the env may
        // own the property and take the write — with/12.10-0-7), so the local
        // starts as a HOLE sentinel and reads check it.
        let id = ctx.define_sloppy_implicit_global(name.to_string());
        ctx.with_sloppy_implicit_ids.insert(id, name.to_string());
        ctx.pending_with_implicit_inits.push((id, name.to_string()));
        WithSetFallback::SloppyImplicit(id)
    }
}

pub(crate) fn anonymous_class_has_static_name_member(class: &ast::Class) -> bool {
    class.body.iter().any(|member| match member {
        ast::ClassMember::Method(method) if method.is_static => {
            matches!(&method.key, ast::PropName::Ident(ident) if ident.sym.as_ref() == "name")
                || matches!(&method.key, ast::PropName::Str(s) if s.value.as_str() == Some("name"))
        }
        ast::ClassMember::ClassProp(prop) if prop.is_static => {
            matches!(&prop.key, ast::PropName::Ident(ident) if ident.sym.as_ref() == "name")
                || matches!(&prop.key, ast::PropName::Str(s) if s.value.as_str() == Some("name"))
        }
        _ => false,
    })
}

/// True when an `Expr` is cheap to evaluate more than once with no observable
/// side effects — safe to duplicate into an optional-call guard condition.
/// Conservative: only the obvious read-only leaf/access shapes qualify.
pub(crate) fn opt_call_receiver_repeatable(expr: &Expr) -> bool {
    match expr {
        Expr::LocalGet(_)
        | Expr::GlobalGet(_)
        | Expr::This
        | Expr::Undefined
        | Expr::Null
        | Expr::Number(_)
        | Expr::String(_)
        | Expr::Bool(_) => true,
        // `process.env` and env-var reads are pure, side-effect-free, and
        // stable within an expression, so they are safe to evaluate more than
        // once (the guard AND the call). Without this, `process.env?.[k]?.m()`
        // has a NON-repeatable receiver `IndexGet { object: ProcessEnv, .. }`,
        // so the optional-method null-guard is dropped and `.m()` is called on
        // the unguarded `undefined` an unset var reads as — e.g.
        // `process.env.ANTHROPIC_BASE_URL?.trim()` returned the string
        // "undefined" instead of short-circuiting to `undefined`.
        Expr::ProcessEnv | Expr::EnvGet(_) => true,
        Expr::EnvGetDynamic(key) => opt_call_receiver_repeatable(key),
        // `a.b` / `a[const]` chains over repeatable receivers stay repeatable
        // (property reads are not side-effecting in this codebase's model).
        Expr::PropertyGet { object, .. } => opt_call_receiver_repeatable(object),
        Expr::IndexGet { object, index } => {
            opt_call_receiver_repeatable(object) && opt_call_receiver_repeatable(index)
        }
        _ => false,
    }
}

/// Build the condition under which `obj.method?.(args)` short-circuits to
/// `undefined`: the resolved function value is nullish. The naive check
/// `obj.method == null` is WRONG when `obj` is a primitive string, because
/// `PropertyGet{string, method}` reads back `undefined` even though the
/// builtin (`split`/`replace`/…) is perfectly callable through the call path
/// — so the guard wrongly short-circuited (`mime`'s
/// `type?.split?.(';')[0]` returned `undefined`). Per spec, a string DOES have
/// the method, so we must NOT short-circuit. When the receiver is repeatable
/// we widen the guard to `func_value == null && typeof receiver !== "string"`:
/// for a real string the typeof clause is false (never short-circuit → the
/// call dispatches the builtin), while a user object missing the method still
/// short-circuits (#830 preserved). Non-repeatable receivers keep the plain
/// function-value check to avoid double-evaluating side effects.
pub(crate) fn opt_call_func_nullish_guard(receiver: &Expr, func_value: Expr) -> Expr {
    let func_nullish = Expr::Compare {
        op: CompareOp::LooseEq,
        left: Box::new(func_value),
        right: Box::new(Expr::Null),
    };
    if opt_call_receiver_repeatable(receiver) {
        let not_string = Expr::Compare {
            op: CompareOp::Ne,
            left: Box::new(Expr::TypeOf(Box::new(receiver.clone()))),
            right: Box::new(Expr::String("string".to_string())),
        };
        Expr::Logical {
            op: LogicalOp::And,
            left: Box::new(func_nullish),
            right: Box::new(not_string),
        }
    } else {
        func_nullish
    }
}

/// Lower a bare identifier that is bound to a native module (via a named or
/// namespace import — `import { relative } from 'path'`, `import * as os from
/// 'os'`) to the value-expression it denotes.
///
/// Used both from the identifier expression path and from object-literal
/// shorthand resolution (`{ relative }` — #5242), so a native-module-bound
/// name produces the same callable/property value whether it appears as a
/// standalone reference or as a shorthand property. The caller must ensure
/// `ctx.lookup_native_module(name)` is `Some`.
pub(crate) fn native_module_binding_value(ctx: &LoweringContext, name: &str) -> Expr {
    let (module_name, method_name) = match ctx.lookup_native_module(name) {
        Some(v) => v,
        None => return Expr::Undefined,
    };
    if module_name == "os" || module_name == "node:os" {
        if let Some(method) = method_name {
            match method {
                "EOL" => return Expr::OsEOL,
                "devNull" => return Expr::OsDevNull,
                _ => {}
            }
        }
    }
    if module_name == "buffer" || module_name == "node:buffer" {
        if let Some(method) = method_name {
            if matches!(method, "constants" | "kMaxLength" | "kStringMaxLength") {
                return Expr::PropertyGet {
                    object: Box::new(Expr::NativeModuleRef("buffer".to_string())),
                    property: method.to_string(),
                };
            }
        }
    }
    // Special handling for worker_threads named imports
    if module_name == "worker_threads" {
        if let Some(method) = method_name {
            if method == "workerData" {
                return Expr::PropertyGet {
                    object: Box::new(Expr::NativeModuleRef("worker_threads".to_string())),
                    property: "workerData".to_string(),
                };
            }
        }
    }
    if let Some(method) = method_name {
        // #3946: a `node:process` *property* imported by name
        // (`import { pid, arch } from "node:process"`) must read
        // the live process value, not a generic native-module
        // PropertyGet (which resolved to `undefined`). Methods
        // fall through to the callable native-module ref below.
        if module_name == "process" {
            if let Some(e) = expr_member::lower_process_named_property(method) {
                return e;
            }
        }
        return Expr::PropertyGet {
            object: Box::new(Expr::NativeModuleRef(module_name.to_string())),
            property: method.to_string(),
        };
    }
    if ctx.lookup_builtin_module_alias(name).is_none()
        && is_cjs_style_native_default_import(module_name)
    {
        return Expr::PropertyGet {
            object: Box::new(Expr::NativeModuleRef(module_name.to_string())),
            property: "default".to_string(),
        };
    }
    // Native module reference (e.g., mysql from 'mysql2/promise')
    Expr::NativeModuleRef(module_name.to_string())
}

pub(crate) fn expr_uses_stack_heavy_chain_lowering(expr: &ast::Expr) -> bool {
    matches!(expr, ast::Expr::Bin(_) | ast::Expr::Member(_))
}

/// Re-lowering diagnostics, fully gated behind the `PERRY_TRACE_RELOWER` env
/// var (zero overhead unless set). Counts every `lower_expr` invocation keyed
/// by source span, so a span lowered far more than once flags redundant
/// re-lowering (the classic source of super-linear HIR-lowering blowup on
/// minified bundles). On every N-million calls — and so still on a kill — it
/// dumps the total/distinct counts and the top re-lowered spans to stderr.
/// Kept (env-gated) as a standing diagnostic for future lowering perf work.
pub(crate) mod relower_trace {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    static ENABLED: AtomicBool = AtomicBool::new(false);
    static INIT: AtomicBool = AtomicBool::new(false);
    static TOTAL: AtomicU64 = AtomicU64::new(0);

    thread_local! {
        static SPANS: RefCell<HashMap<(u32, u32), u32>> = RefCell::new(HashMap::new());
    }

    pub fn enabled() -> bool {
        if !INIT.load(Ordering::Relaxed) {
            let on = std::env::var("PERRY_TRACE_RELOWER").is_ok();
            ENABLED.store(on, Ordering::Relaxed);
            INIT.store(true, Ordering::Relaxed);
        }
        ENABLED.load(Ordering::Relaxed)
    }

    pub fn record(lo: u32, hi: u32) {
        let n = TOTAL.fetch_add(1, Ordering::Relaxed) + 1;
        SPANS.with(|m| {
            *m.borrow_mut().entry((lo, hi)).or_insert(0) += 1;
        });
        if n.is_multiple_of(5_000_000) {
            dump(&format!("periodic@{n}"));
        }
    }

    fn dump(tag: &str) {
        SPANS.with(|m| {
            let m = m.borrow();
            let total = TOTAL.load(Ordering::Relaxed);
            let distinct = m.len();
            let mut v: Vec<_> = m.iter().map(|(k, c)| (*c, *k)).collect();
            v.sort_unstable_by(|a, b| b.0.cmp(&a.0));
            eprintln!(
                "RELOWER[{tag}] total={total} distinct={distinct} ratio={:.2}",
                total as f64 / distinct.max(1) as f64
            );
            for (c, (lo, hi)) in v.into_iter().take(20) {
                eprintln!("RELOWER  span {lo}..{hi} count={c}");
            }
        });
    }
}

pub(crate) fn lower_expr_with_json_parse_type_hint(
    ctx: &mut LoweringContext,
    expr: &ast::Expr,
    ts_type: &ast::TsType,
) -> Result<Expr> {
    let lowered = lower_expr(ctx, expr)?;
    let Expr::JsonParse(text) = lowered else {
        return Ok(lowered);
    };

    // Preserve the common `JSON.parse(blob) as T` type hint in HIR, matching
    // the existing `JSON.parse<T>(blob)` path. The assertion still erases at
    // runtime; this only gives codegen the same opportunity to choose a
    // specialized parse path when the target type is concrete enough.
    let ty = extract_ts_type_with_ctx(ts_type, Some(ctx));
    let resolved = resolve_typed_parse_ty(ctx, ty);
    if matches!(resolved, Type::Any | Type::Unknown) || !typed_parse_codegen_supports(&resolved) {
        return Ok(Expr::JsonParse(text));
    }

    Ok(Expr::JsonParseTyped {
        text,
        ty: resolved,
        ordered_keys: extract_typed_parse_source_order(ts_type, ctx),
    })
}

pub(crate) fn typed_parse_codegen_supports(ty: &Type) -> bool {
    let elem = match ty {
        Type::Array(inner) => inner.as_ref(),
        Type::Generic { base, type_args } if base == "Array" && type_args.len() == 1 => {
            &type_args[0]
        }
        _ => return false,
    };

    matches!(elem, Type::Object(obj) if !obj.properties.is_empty())
}
