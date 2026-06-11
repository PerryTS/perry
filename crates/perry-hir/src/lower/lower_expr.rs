//! AST `Expr` → HIR `Expr` lowering: `lower_expr`, `lower_expr_assignment`,
//! and the `Text` reactive-template desugar helper.
//!
//! Extracted from `lower/mod.rs` so the entry-point file stays under the
//! 2,000-LOC soft cap. The `match expr` body inside `lower_expr` still
//! delegates the larger variant arms to existing sibling modules
//! (`expr_call`, `expr_member`, `expr_assign`, `expr_function`,
//! `expr_object`, `expr_new`, `expr_misc`); this file holds only the
//! `match` skeleton and the smaller inline arms (`Ident`, `Bin`,
//! `Unary`, `Array`, `OptChain`, `TaggedTpl`, `Class`, the TS
//! pass-through assertions).
//!
//! Visibility note: `lower_expr_assignment` and `try_desugar_reactive_text`
//! were `pub(super)` — bumped to `pub(crate)` so the mod.rs named
//! re-exports can propagate them.

use anyhow::{anyhow, Result};
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use super::*;
use crate::ir::*;
use crate::lower_types::extract_ts_type_with_ctx;

fn class_computed_member_registration_expr(class_name: &str, member: &ClassComputedMember) -> Expr {
    match member.kind {
        ClassComputedMemberKind::Method => Expr::RegisterClassComputedMethod {
            class_name: class_name.to_string(),
            key_expr: Box::new(member.key_expr.clone()),
            method_name: member.function.name.clone(),
            is_static: member.is_static,
            param_count: member.function.params.len() as u32,
            has_rest: member
                .function
                .params
                .last()
                .map(|p| p.is_rest)
                .unwrap_or(false),
        },
        ClassComputedMemberKind::Getter => Expr::RegisterClassComputedAccessor {
            class_name: class_name.to_string(),
            key_expr: Box::new(member.key_expr.clone()),
            getter_name: Some(member.function.name.clone()),
            setter_name: None,
            is_static: member.is_static,
        },
        ClassComputedMemberKind::Setter => Expr::RegisterClassComputedAccessor {
            class_name: class_name.to_string(),
            key_expr: Box::new(member.key_expr.clone()),
            getter_name: None,
            setter_name: Some(member.function.name.clone()),
            is_static: member.is_static,
        },
    }
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
    }
}

fn is_known_global_identifier_name(name: &str) -> bool {
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

fn is_fetch_global_value_name(name: &str) -> bool {
    matches!(
        name,
        "fetch" | "Blob" | "File" | "FormData" | "Headers" | "Request" | "Response"
    )
}

fn is_cjs_style_native_default_import(module_name: &str) -> bool {
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

fn wrap_with_gets(property: &str, fallback: Expr, envs: Vec<LocalId>) -> Expr {
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

fn anonymous_class_has_static_name_member(class: &ast::Class) -> bool {
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

pub(crate) fn lower_expr_assignment(
    ctx: &mut LoweringContext,
    expr: &ast::Expr,
    value: Box<Expr>,
) -> Result<Expr> {
    match expr {
        ast::Expr::Ident(ident) => {
            let name = ident.sym.to_string();
            if let Some(env_id) = ctx.active_with_envs_for_ident(&name).into_iter().next() {
                let fallback = with_set_fallback_for_ident(ctx, &name);
                return Ok(Expr::WithSet {
                    object: Box::new(Expr::LocalGet(env_id)),
                    property: name,
                    value,
                    fallback,
                    strict: ctx.current_strict,
                });
            }
            if let Some(id) = ctx.lookup_local(&name) {
                Ok(Expr::LocalSet(id, value))
            } else if ctx.lookup_class(&name).is_some() || ctx.lookup_func(&name).is_some() {
                // v0.5.757: don't shadow a class/function binding with an
                // implicit local for `<Name> = X` patterns. Drizzle's
                // sql.js uses `((sql2) => { ... })(sql || (sql = {}))` —
                // the binding exists (truthy), the OR short-circuits, and
                // the assignment is dead. Pre-fix the implicit local hid
                // the original binding from later reads. Just evaluate
                // the RHS for side effects. Refs #420.
                Ok(*value)
            } else {
                if ctx.current_strict {
                    return Ok(Expr::Sequence(vec![
                        *value,
                        throw_reference_error_expr(
                            "js_throw_reference_error_unresolved_assignment",
                        ),
                    ]));
                }
                eprintln!(
                    "  Warning: Assignment to undeclared variable '{}', creating sloppy global",
                    name
                );
                // Sloppy implicit global: the binding IS a property of
                // globalThis (spec CreateGlobalVarBinding on the global
                // object), so `foo = 1` must be visible as
                // `globalThis.foo`, write through to a pre-existing global
                // property, and observe a later `delete globalThis.foo`.
                // Reads of the name resolve through the
                // `js_global_get_or_throw_unresolved` fallback, so no
                // module-local shadow may be created here (a stale local
                // would keep serving deleted/overwritten values).
                // NOTE: `GlobalGet(0)` alone is a by-name routing SENTINEL in
                // codegen (bare reads lower to 0.0) — the write must target
                // the VALUE globalThis, which the `PropertyGet { GlobalGet(0),
                // "globalThis" }` shape resolves to the real global object.
                Ok(Expr::PropertySet {
                    object: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::GlobalGet(0)),
                        property: "globalThis".to_string(),
                    }),
                    property: name,
                    value,
                })
            }
        }
        ast::Expr::Member(member) => {
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                let obj_name = obj_ident.sym.to_string();
                if ctx.lookup_class(&obj_name).is_some() {
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        let field_name = prop_ident.sym.to_string();
                        if ctx.has_static_field(&obj_name, &field_name) {
                            return Ok(Expr::StaticFieldSet {
                                class_name: obj_name,
                                field_name,
                                value,
                            });
                        }
                    }
                }
            }
            let object_expr = lower_expr(ctx, &member.obj)?;
            let object = Box::new(object_expr.clone());
            match &member.prop {
                ast::MemberProp::Ident(ident) => {
                    let property = ident.sym.to_string();
                    // Issue #711 part 2: `<expr>.prototype = <value>`
                    // pattern (Effect's effectable.ts uses this to
                    // declare prototype-based classes — `function
                    // Base() {}; Base.prototype = CommitPrototype`).
                    // Route through the SetFunctionPrototype HIR node
                    // so codegen calls
                    // `js_set_function_prototype(func, proto)`, which
                    // allocates a synthetic class id keyed by the
                    // function value. The runtime helper is a no-op
                    // when `object` doesn't evaluate to a function
                    // (preserves baseline for legitimate
                    // `someClass.prototype = X` writes on non-function
                    // values).
                    if property == "prototype" {
                        return Ok(Expr::SetFunctionPrototype {
                            func: object,
                            proto: value,
                        });
                    }
                    Ok(Expr::PutValueSet {
                        target: object.clone(),
                        key: Box::new(Expr::String(property)),
                        value,
                        receiver: object,
                        strict: ctx.current_strict,
                    })
                }
                ast::MemberProp::Computed(computed) => {
                    let index = Box::new(lower_expr(ctx, &computed.expr)?);
                    Ok(Expr::PutValueSet {
                        target: object.clone(),
                        key: index,
                        value,
                        receiver: object,
                        strict: ctx.current_strict,
                    })
                }
                ast::MemberProp::PrivateName(private) => {
                    let property = format!("#{}", private.name);
                    let object = expr_member::wrap_private_guard(
                        ctx,
                        object,
                        &property,
                        expr_member::PRIV_OP_WRITE,
                    );
                    Ok(Expr::PropertySet {
                        object,
                        property,
                        value,
                    })
                }
            }
        }
        // Recursively unwrap parens and type annotations
        ast::Expr::Paren(paren) => lower_expr_assignment(ctx, &paren.expr, value),
        ast::Expr::TsAs(ts_as) => lower_expr_assignment(ctx, &ts_as.expr, value),
        ast::Expr::TsNonNull(ts_nn) => lower_expr_assignment(ctx, &ts_nn.expr, value),
        ast::Expr::TsTypeAssertion(ts_ta) => lower_expr_assignment(ctx, &ts_ta.expr, value),
        ast::Expr::TsSatisfies(ts_sat) => lower_expr_assignment(ctx, &ts_sat.expr, value),
        _ => Err(anyhow!(
            "Unsupported expression as assignment target: {:?}",
            expr
        )),
    }
}

pub(crate) fn lower_expr(ctx: &mut LoweringContext, expr: &ast::Expr) -> Result<Expr> {
    match expr {
        ast::Expr::Lit(lit) => lower_lit(lit),
        ast::Expr::Ident(ident) => {
            let name = ident.sym.to_string();
            let with_envs = ctx.active_with_envs_for_ident(&name);
            if !with_envs.is_empty() {
                let saved_with_envs = std::mem::take(&mut ctx.with_env_stack);
                let fallback = lower_expr(ctx, expr);
                ctx.with_env_stack = saved_with_envs;
                return Ok(wrap_with_gets(&name, fallback?, with_envs));
            }
            if let Some(id) = ctx.lookup_local(&name) {
                // A with-fallback implicit global may still be the HOLE
                // sentinel (the with-env took the write) — reading it then
                // is a ReferenceError, not undefined.
                if let Some(n) = ctx.with_sloppy_implicit_ids.get(&id) {
                    return Ok(Expr::Call {
                        callee: Box::new(Expr::ExternFuncRef {
                            name: "js_with_implicit_read".to_string(),
                            param_types: vec![Type::Any, Type::String],
                            return_type: Type::Any,
                        }),
                        args: vec![Expr::LocalGet(id), Expr::String(n.clone())],
                        type_args: vec![],
                    });
                }
                Ok(Expr::LocalGet(id))
            } else if let Some(id) = ctx.lookup_func(&name) {
                Ok(Expr::FuncRef(id))
            } else if let Some((module_name, method_name)) = ctx.lookup_native_module(&name) {
                if module_name == "os" || module_name == "node:os" {
                    if let Some(method) = method_name {
                        match method {
                            "EOL" => return Ok(Expr::OsEOL),
                            "devNull" => return Ok(Expr::OsDevNull),
                            _ => {}
                        }
                    }
                }
                if module_name == "buffer" || module_name == "node:buffer" {
                    if let Some(method) = method_name {
                        if matches!(method, "constants" | "kMaxLength" | "kStringMaxLength") {
                            return Ok(Expr::PropertyGet {
                                object: Box::new(Expr::NativeModuleRef("buffer".to_string())),
                                property: method.to_string(),
                            });
                        }
                    }
                }
                // Special handling for worker_threads named imports
                if module_name == "worker_threads" {
                    if let Some(method) = method_name {
                        if method == "workerData" {
                            return Ok(Expr::PropertyGet {
                                object: Box::new(Expr::NativeModuleRef(
                                    "worker_threads".to_string(),
                                )),
                                property: "workerData".to_string(),
                            });
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
                            return Ok(e);
                        }
                    }
                    return Ok(Expr::PropertyGet {
                        object: Box::new(Expr::NativeModuleRef(module_name.to_string())),
                        property: method.to_string(),
                    });
                }
                if ctx.lookup_builtin_module_alias(&name).is_none()
                    && is_cjs_style_native_default_import(module_name)
                {
                    return Ok(Expr::PropertyGet {
                        object: Box::new(Expr::NativeModuleRef(module_name.to_string())),
                        property: "default".to_string(),
                    });
                }
                // Native module reference (e.g., mysql from 'mysql2/promise')
                Ok(Expr::NativeModuleRef(module_name.to_string()))
            } else if let Some(orig_name) = ctx.lookup_imported_func(&name) {
                // Imported function - reference by its original exported name
                // Look up type information if available
                let (param_types, return_type) = ctx
                    .lookup_extern_func_types(orig_name)
                    .map(|(p, r)| (p.clone(), r.clone()))
                    .unwrap_or_else(|| (Vec::new(), Type::Any));
                Ok(Expr::ExternFuncRef {
                    name: orig_name.to_string(),
                    param_types,
                    return_type,
                })
            } else if is_builtin_function(&name) {
                // Built-in global function (setTimeout, etc.)
                Ok(Expr::ExternFuncRef {
                    name,
                    param_types: Vec::new(),
                    return_type: Type::Any,
                })
            } else if ctx.lookup_class(&name).is_some() {
                // Class used as a first-class value (e.g., { Point: Point })
                Ok(Expr::ClassRef(name))
            } else if name == "undefined" {
                // Global undefined identifier
                Ok(Expr::Undefined)
            } else if name == "null" {
                // Global null identifier (though typically written as literal)
                Ok(Expr::Null)
            } else if name == "NaN" {
                // Global NaN identifier
                Ok(Expr::Number(f64::NAN))
            } else if name == "Infinity" {
                // Global Infinity identifier
                Ok(Expr::Number(f64::INFINITY))
            } else if name == "__dirname" || name == "__filename" {
                // Issue #667: CJS-style module locals. Without this fold,
                // the bare reference falls through to GlobalGet(0) -> 0,
                // which silently corrupts any path computation built on
                // path.join(__dirname, ...). Mirrors the import.meta arm
                // (expr_misc::import_meta_paths) so both surfaces agree.
                let path = ctx.source_file_path.replace('\\', "/");
                let value = if name == "__filename" {
                    path.clone()
                } else {
                    match path.rfind('/') {
                        Some(i) if i > 0 => path[..i].to_string(),
                        Some(_) => "/".to_string(),
                        None => String::new(),
                    }
                };
                Ok(Expr::String(value))
            } else if matches!(name.as_str(), "Math" | "JSON" | "Reflect" | "Intl") {
                // #4139: the built-in namespace objects used as VALUES (passed
                // to `Object.getOwnPropertyDescriptor(Math, …)`, stored in a
                // local, etc.) must resolve to the real
                // `populate_global_this_builtins`-installed namespace object —
                // not the bare `GlobalGet(0)` sentinel (which IS `globalThis`,
                // so `Math === globalThis` and reflection reads the wrong
                // object). Reuse the `PropertyGet { GlobalGet(0), <name> }`
                // value-form (same as the built-in constructors above). When
                // these names appear in member-OBJECT position (`Math.max(…)`,
                // `Math.PI`), expr_member.rs's #973 reroute-undo resets the
                // receiver back to `GlobalGet(0)`, so the intrinsic call /
                // constant-fold paths are unchanged. A shadowing local would
                // have matched `ctx.lookup_local` earlier and never reached
                // here.
                Ok(Expr::PropertyGet {
                    object: Box::new(Expr::GlobalGet(0)),
                    property: name,
                })
            } else {
                // GlobalGet(0) is a sentinel: codegen routes by name from the
                // parent PropertyGet/Call/Member context. Bare uses lower to
                // 0.0 (perry-codegen/src/expr.rs Expr::GlobalGet arm).
                let known_global = is_known_global_identifier_name(&name);
                if !known_global && !ctx.unresolved_ident_as_global {
                    // A global created at RUNTIME (sloppy `this.y = 2` with
                    // `this` = globalThis inside a dynamic function) is
                    // invisible to compile-time resolution — look it up on
                    // globalThis first; only a true miss throws the spec
                    // ReferenceError, with the identifier in the message.
                    return Ok(Expr::Call {
                        callee: Box::new(Expr::ExternFuncRef {
                            name: "js_global_get_or_throw_unresolved".to_string(),
                            param_types: vec![Type::Any],
                            return_type: Type::Any,
                        }),
                        args: vec![Expr::String(name.clone())],
                        type_args: Vec::new(),
                    });
                }
                if !known_global {
                    eprintln!(
                        "  Warning: unknown identifier '{}' — assuming global; member access will dispatch by name at runtime, bare reads lower to 0",
                        name
                    );
                }
                // Bare built-in constructor identifiers (`Date`, `Array`,
                // `Object`, ...) used as VALUES (not method receivers /
                // `new` callees) need a real closure pointer so identity
                // comparisons like `inst.constructor === Date` hold —
                // both sides must resolve to the same `populate_global_this_builtins`-
                // installed closure. Reuse the existing
                // `PropertyGet { GlobalGet, <name> }` codegen path that
                // dispatches through `js_get_global_this` for builtin
                // names. Bare-callee shapes (e.g. `Date.now()`, `new
                // Date()`) are picked off earlier by their dedicated HIR
                // variants — `Expr::DateNow`, `Expr::DateNew(...)`,
                // `Expr::Date*Get(...)` — so they don't reach this arm.
                // date-fns / drizzle / lodash duck-typing path.
                if is_builtin_global_value_name(&name) {
                    if is_fetch_global_value_name(&name) {
                        ctx.uses_fetch = true;
                    }
                    return Ok(Expr::PropertyGet {
                        object: Box::new(Expr::GlobalGet(0)),
                        property: name,
                    });
                }
                Ok(Expr::GlobalGet(0))
            }
        }
        ast::Expr::Bin(bin) => {
            // Handle 'in' operator: property in object
            if matches!(bin.op, ast::BinaryOp::In) {
                if let ast::Expr::PrivateName(private) = bin.left.as_ref() {
                    let class_name = ctx.current_class.clone().ok_or_else(|| {
                        anyhow!("Private name brand check is only supported inside a class")
                    })?;
                    let field_name = format!("#{}", private.name);
                    let object = Box::new(lower_expr(ctx, &bin.right)?);
                    return Ok(Expr::PrivateBrandCheck {
                        class_name,
                        field_name,
                        object,
                    });
                }
                // Proxy fast path: `key in proxy` routes through js_proxy_has.
                if let ast::Expr::Ident(obj_ident) = bin.right.as_ref() {
                    let obj_name = obj_ident.sym.to_string();
                    if ctx.proxy_locals.contains(&obj_name) {
                        let key = Box::new(lower_expr(ctx, &bin.left)?);
                        let proxy = Box::new(lower_expr(ctx, &bin.right)?);
                        return Ok(Expr::ProxyHas { proxy, key });
                    }
                }
                let property = Box::new(lower_expr(ctx, &bin.left)?);
                let object = Box::new(lower_expr(ctx, &bin.right)?);
                return Ok(Expr::In { property, object });
            }

            // Handle instanceof specially - needs to extract class name
            if matches!(bin.op, ast::BinaryOp::InstanceOf) {
                // WeakRef / FinalizationRegistry: pre-scan tracks local
                // constructor results explicitly, so common `local instanceof
                // WeakRef|FinalizationRegistry` checks can be folded at
                // lowering time when we recognise the receiver.
                if let ast::Expr::Ident(class_ident) = bin.right.as_ref() {
                    let class_name = class_ident.sym.as_ref();
                    if class_name == "WeakRef" || class_name == "FinalizationRegistry" {
                        if let ast::Expr::Ident(left_ident) = bin.left.as_ref() {
                            let local_name = left_ident.sym.to_string();
                            let is_match = (class_name == "WeakRef"
                                && ctx.weakref_locals.contains(&local_name))
                                || (class_name == "FinalizationRegistry"
                                    && ctx.finreg_locals.contains(&local_name));
                            return Ok(Expr::Bool(is_match));
                        }
                    }
                }
                let expr = Box::new(lower_expr(ctx, &bin.left)?);
                // Right side can be an identifier (ClassName) or member expression (Module.ClassName)
                let ty = match bin.right.as_ref() {
                    ast::Expr::Ident(ident) => ident.sym.to_string(),
                    ast::Expr::Member(member) => {
                        // Handle Module.ClassName - extract the full qualified name
                        let obj_name = if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                            obj_ident.sym.to_string()
                        } else {
                            "Unknown".to_string()
                        };
                        let prop_name = match &member.prop {
                            ast::MemberProp::Ident(prop_ident) => prop_ident.sym.to_string(),
                            _ => "Unknown".to_string(),
                        };
                        format!("{}.{}", obj_name, prop_name)
                    }
                    _ => {
                        // For complex expressions, use a generic type name
                        "Object".to_string()
                    }
                };
                // v0.5.749: when the right side resolves to a local
                // variable holding a class ref (e.g. `function is(value,
                // type) { return value instanceof type; }`), emit a
                // dynamic-dispatch path that evaluates the class ref at
                // runtime. Without this, the codegen sees `ty = "type"`
                // (the param name), can't resolve it as a class, and
                // falls through to `class_id = 0` — every dynamic
                // instanceof returns false. Drizzle's `is(value, type)`
                // chain depends on this. Refs #420 / #618 followup.
                let ty_expr = match bin.right.as_ref() {
                    ast::Expr::Ident(ident) => {
                        let name = ident.sym.as_ref();
                        // `x instanceof undefined`: `undefined` is the primitive
                        // value, never a class name. Codegen would resolve `ty =
                        // "undefined"` to class_id 0 and silently return `false`;
                        // ECMAScript requires evaluating the RHS and throwing a
                        // TypeError because it is not an object (test262
                        // instanceof/S11.8.6_A3 #4). Lower it to the undefined
                        // value so it routes through `js_instanceof_dynamic`.
                        if name == "undefined" {
                            Some(Box::new(Expr::Undefined))
                        } else
                        // A local holding a class ref (drizzle's `is(value, type)`),
                        // OR a top-level ES5 function constructor (`function Foo(){…}`
                        // used as `x instanceof Foo`). The latter has no class entry,
                        // so without a dynamic value codegen resolves `ty = "Foo"` to
                        // class_id 0 and instanceof always returns false — which makes
                        // the ubiquitous `if (!(this instanceof Foo)) return new Foo()`
                        // guard recurse forever. Lower the function to its value and
                        // route through `js_instanceof_dynamic`, which derives the same
                        // `synthetic_class_id_for_function` that `new Foo()` stamps onto
                        // the instance (see js_new_function_construct).
                        if ctx.lookup_local(name).is_some()
                            || ctx.lookup_func(name).is_some()
                            || ctx.lookup_native_module(name).is_some()
                        {
                            match lower_expr(ctx, &bin.right) {
                                Ok(e) => Some(Box::new(e)),
                                Err(_) => None,
                            }
                        } else {
                            None
                        }
                    }
                    ast::Expr::Member(member) => {
                        let native_module = if let ast::Expr::Ident(obj_ident) = member.obj.as_ref()
                        {
                            let obj_name = obj_ident.sym.as_ref();
                            // `Temporal.<X>` constructors dispatch via brand arms,
                            // not a class chain, so route them through the runtime
                            // dynamic path (`js_instanceof_dynamic` →
                            // `temporal_ctor_kind`) by lowering the constructor to
                            // its closure value here.
                            obj_name == "Temporal"
                                || ctx.lookup_builtin_module_alias(obj_name).is_some()
                                || matches!(ctx.lookup_native_module(obj_name), Some((_, None)))
                        } else {
                            false
                        };
                        if native_module {
                            match lower_expr(ctx, &bin.right) {
                                Ok(e) => Some(Box::new(e)),
                                Err(_) => None,
                            }
                        } else {
                            None
                        }
                    }
                    // Any other right-hand side (a primitive literal like
                    // `x instanceof true`, `this`, a call `x instanceof f()`,
                    // a parenthesized/conditional class ref, …) is NOT a
                    // statically-resolvable class name. The old `_ => "Object"`
                    // `ty` substitution silently treated these as
                    // `instanceof Object` and returned `false`; ECMAScript
                    // requires evaluating the operand and throwing a TypeError
                    // when it is not a constructor (`true instanceof true`,
                    // `({}) instanceof this`). Lower the operand to a value and
                    // route through `js_instanceof_dynamic`, which both resolves
                    // every constructor shape and throws on a non-callable RHS.
                    _ => match lower_expr(ctx, &bin.right) {
                        Ok(e) => Some(Box::new(e)),
                        Err(_) => None,
                    },
                };
                return Ok(Expr::InstanceOf { expr, ty, ty_expr });
            }

            let left = Box::new(lower_expr(ctx, &bin.left)?);
            let right = Box::new(lower_expr(ctx, &bin.right)?);

            match bin.op {
                // Arithmetic
                ast::BinaryOp::Add => Ok(Expr::Binary {
                    op: BinaryOp::Add,
                    left,
                    right,
                }),
                ast::BinaryOp::Sub => Ok(Expr::Binary {
                    op: BinaryOp::Sub,
                    left,
                    right,
                }),
                ast::BinaryOp::Mul => Ok(Expr::Binary {
                    op: BinaryOp::Mul,
                    left,
                    right,
                }),
                ast::BinaryOp::Div => Ok(Expr::Binary {
                    op: BinaryOp::Div,
                    left,
                    right,
                }),
                ast::BinaryOp::Mod => Ok(Expr::Binary {
                    op: BinaryOp::Mod,
                    left,
                    right,
                }),
                ast::BinaryOp::Exp => Ok(Expr::Binary {
                    op: BinaryOp::Pow,
                    left,
                    right,
                }),

                // Comparison (treat == same as === for typed code)
                ast::BinaryOp::EqEq => {
                    // Proxy/Reflect fold: `Reflect.getPrototypeOf(x) === <Class>.prototype`
                    // always true in our model (we don't maintain real prototypes).
                    // Same fold for `Object.getPrototypeOf(x) === <Class>.prototype`.
                    if matches!(
                        &*left,
                        Expr::ReflectGetPrototypeOf(_) | Expr::ObjectGetPrototypeOf(_)
                    ) && matches!(&*right, Expr::PropertyGet { property, .. } if property == "prototype")
                    {
                        return Ok(Expr::Bool(true));
                    }
                    Ok(Expr::Compare {
                        op: CompareOp::LooseEq,
                        left,
                        right,
                    })
                }
                ast::BinaryOp::EqEqEq => {
                    if matches!(
                        &*left,
                        Expr::ReflectGetPrototypeOf(_) | Expr::ObjectGetPrototypeOf(_)
                    ) && matches!(&*right, Expr::PropertyGet { property, .. } if property == "prototype")
                    {
                        return Ok(Expr::Bool(true));
                    }
                    Ok(Expr::Compare {
                        op: CompareOp::Eq,
                        left,
                        right,
                    })
                }
                ast::BinaryOp::NotEq => Ok(Expr::Compare {
                    op: CompareOp::LooseNe,
                    left,
                    right,
                }),
                ast::BinaryOp::NotEqEq => Ok(Expr::Compare {
                    op: CompareOp::Ne,
                    left,
                    right,
                }),
                ast::BinaryOp::Lt => Ok(Expr::Compare {
                    op: CompareOp::Lt,
                    left,
                    right,
                }),
                ast::BinaryOp::LtEq => Ok(Expr::Compare {
                    op: CompareOp::Le,
                    left,
                    right,
                }),
                ast::BinaryOp::Gt => Ok(Expr::Compare {
                    op: CompareOp::Gt,
                    left,
                    right,
                }),
                ast::BinaryOp::GtEq => Ok(Expr::Compare {
                    op: CompareOp::Ge,
                    left,
                    right,
                }),

                // Logical
                ast::BinaryOp::LogicalAnd => Ok(Expr::Logical {
                    op: LogicalOp::And,
                    left,
                    right,
                }),
                ast::BinaryOp::LogicalOr => Ok(Expr::Logical {
                    op: LogicalOp::Or,
                    left,
                    right,
                }),
                ast::BinaryOp::NullishCoalescing => Ok(Expr::Logical {
                    op: LogicalOp::Coalesce,
                    left,
                    right,
                }),

                // Bitwise
                ast::BinaryOp::BitAnd => Ok(Expr::Binary {
                    op: BinaryOp::BitAnd,
                    left,
                    right,
                }),
                ast::BinaryOp::BitOr => Ok(Expr::Binary {
                    op: BinaryOp::BitOr,
                    left,
                    right,
                }),
                ast::BinaryOp::BitXor => Ok(Expr::Binary {
                    op: BinaryOp::BitXor,
                    left,
                    right,
                }),
                ast::BinaryOp::LShift => Ok(Expr::Binary {
                    op: BinaryOp::Shl,
                    left,
                    right,
                }),
                ast::BinaryOp::RShift => Ok(Expr::Binary {
                    op: BinaryOp::Shr,
                    left,
                    right,
                }),
                ast::BinaryOp::ZeroFillRShift => Ok(Expr::Binary {
                    op: BinaryOp::UShr,
                    left,
                    right,
                }),

                _ => Err(anyhow!("Unsupported binary operator: {:?}", bin.op)),
            }
        }
        ast::Expr::Unary(unary) => {
            // AST-level typeof fold for `typeof Object.<known>` /
            // `typeof Array.<known>`. Lowering the operand would yield a
            // generic property-get on the global Object/Array (which
            // currently returns 0/undefined and makes `=== "function"`
            // checks fail). The static methods are real functions in
            // Node, so fold to the literal "function" string here.
            if matches!(unary.op, ast::UnaryOp::TypeOf) {
                // `typeof(x)` parenthesizes the operand, so the AST-level folds
                // below — which match a bare `Ident` / `Member` — would miss it
                // and fall through to a normal operand lowering. For an
                // unresolved identifier that means `typeof(zzz)` emitted a
                // ReferenceError-throwing get instead of folding to "undefined"
                // (the spec's GetValue-skips-on-typeof rule). Peel transparent
                // `Paren` wrappers so the operand-shape folds see through them.
                let typeof_arg = {
                    let mut e = unary.arg.as_ref();
                    while let ast::Expr::Paren(p) = e {
                        e = p.expr.as_ref();
                    }
                    e
                };
                // #677: bare `typeof Function` — Function is a JS built-in
                // constructor, so typeof is "function". Without this fold,
                // the bare ident lowers to `GlobalGet(0)` and typeof reads
                // "object" via the global-this short-circuit.
                if let ast::Expr::Ident(id) = typeof_arg {
                    if id.sym.as_ref() == "Function" && ctx.lookup_local("Function").is_none() {
                        return Ok(Expr::String("function".to_string()));
                    }
                    // #2874: global `Iterator` (TC39 iterator-helpers) is a
                    // constructor function in Node 22+.
                    if id.sym.as_ref() == "Iterator"
                        && ctx.lookup_local("Iterator").is_none()
                        && ctx.lookup_func("Iterator").is_none()
                    {
                        return Ok(Expr::String("function".to_string()));
                    }
                    // #1454: global timer builtins and fetch are functions.
                    // Timers still lower bare reads to ExternFuncRef; fetch
                    // now resolves through globalThis for value identity.
                    // Fold both shapes to "function" (gc is excluded — it's
                    // undefined in Node without --expose-gc).
                    let n = id.sym.as_ref();
                    if matches!(
                        n,
                        "setTimeout"
                            | "setInterval"
                            | "setImmediate"
                            | "clearTimeout"
                            | "clearInterval"
                            | "clearImmediate"
                            | "fetch"
                            // Callable global helpers that otherwise resolve to
                            // `GlobalGet(0)` (globalThis) for a bare read, so a
                            // value `typeof` reported "object" despite being
                            // fully callable. (#3986)
                            | "queueMicrotask"
                            | "structuredClone"
                            | "btoa"
                            | "atob"
                    ) && ctx.lookup_local(n).is_none()
                    {
                        return Ok(Expr::String("function".to_string()));
                    }
                    // #1535: `import Stream from "node:stream"` should make
                    // `typeof Stream === "function"` (legacy Stream
                    // constructor with class statics hung off it). Perry
                    // resolves the default import to a native-module
                    // namespace today, so the read defaulted to typeof
                    // "object". Fold when the local ident is bound as the
                    // default import of a node module whose default export
                    // Node exposes as a constructor function. (Other
                    // modules whose default is a non-callable namespace —
                    // `node:os`, `node:path` — stay typeof "object".)
                    // Only the DEFAULT import (`import Stream from …`) folds to
                    // "function". A namespace import (`import * as nsStream …`)
                    // also registers as a native module with method `None`, but
                    // it is a module namespace object — `typeof nsStream` must
                    // stay "object" (#1535). Namespace imports additionally
                    // register a builtin-module alias; default imports do not,
                    // so the alias absence is the discriminator.
                    if ctx.lookup_local(n).is_none() && ctx.lookup_builtin_module_alias(n).is_none()
                    {
                        if let Some((module_name, None)) = ctx.lookup_native_module(n) {
                            if matches!(module_name, "stream" | "node:stream") {
                                return Ok(Expr::String("function".to_string()));
                            }
                        }
                    }
                    if ctx.lookup_local(n).is_none()
                        && ctx.lookup_func(n).is_none()
                        && ctx.lookup_native_module(n).is_none()
                        && ctx.lookup_imported_func(n).is_none()
                        && ctx.lookup_class(n).is_none()
                        && !is_builtin_function(n)
                        && !is_known_global_identifier_name(n)
                        && !matches!(n, "undefined" | "null" | "NaN" | "Infinity")
                    {
                        // Not foldable to a compile-time "undefined": sloppy
                        // implicit globals are runtime globalThis properties
                        // (#3575), so `g = 5; typeof g` must observe the live
                        // binding. Non-throwing lookup per the spec's
                        // GetValue-skips-on-typeof rule.
                        return Ok(Expr::TypeOf(Box::new(Expr::Call {
                            callee: Box::new(Expr::ExternFuncRef {
                                name: "js_global_get_optional".to_string(),
                                param_types: vec![Type::Any],
                                return_type: Type::Any,
                            }),
                            args: vec![Expr::String(n.to_string())],
                            type_args: Vec::new(),
                        })));
                    }
                }
                // #1395: `typeof process.memoryUsage.rss` is a nested member
                // (`(process.memoryUsage).rss`) so it bypasses the
                // ident-receiver fold below. Node exposes `rss` as a fast-path
                // function hung off `process.memoryUsage`; fold to "function".
                if let ast::Expr::Member(outer) = typeof_arg {
                    if let ast::MemberProp::Ident(outer_prop) = &outer.prop {
                        if outer_prop.sym.as_ref() == "rss" {
                            if let ast::Expr::Member(inner) = outer.obj.as_ref() {
                                if let (ast::Expr::Ident(root), ast::MemberProp::Ident(mid)) =
                                    (inner.obj.as_ref(), &inner.prop)
                                {
                                    if root.sym.as_ref() == "process"
                                        && mid.sym.as_ref() == "memoryUsage"
                                        && ctx.lookup_local("process").is_none()
                                    {
                                        return Ok(Expr::String("function".to_string()));
                                    }
                                }
                            }
                        }
                    }
                }
                if let ast::Expr::Member(member) = typeof_arg {
                    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                        if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                            let obj_name = obj_ident.sym.as_ref();
                            let prop_name = prop_ident.sym.as_ref();
                            if matches!(prop_name, "encode" | "encodeInto")
                                && ctx
                                    .lookup_local_type(obj_name)
                                    .map(|ty| matches!(ty, Type::Named(name) if name == "TextEncoder"))
                                    .unwrap_or(false)
                            {
                                return Ok(Expr::String("function".to_string()));
                            }
                            if prop_name == "decode"
                                && ctx
                                    .lookup_local_type(obj_name)
                                    .map(|ty| matches!(ty, Type::Named(name) if name == "TextDecoder"))
                                    .unwrap_or(false)
                            {
                                return Ok(Expr::String("function".to_string()));
                            }
                            // #2143: `typeof Promise.resolve`, `typeof Math.min`,
                            // `typeof JSON.parse`, etc. — namespace static methods
                            // that Perry implements as codegen direct-call
                            // intrinsics. A bare value-read of these lowers to a
                            // numeric fallback (typeof "number"), but Node treats
                            // them as real functions. Folding to "function" here
                            // unblocks feature-detection idioms and the
                            // `.bind`/`.call`/`.apply` chain fold below. The
                            // existing Object/Array static method lists are
                            // subsumed by `is_known_namespace_static_function`.
                            if ctx.lookup_local(obj_name).is_none()
                                && ctx.lookup_func(obj_name).is_none()
                                && is_known_namespace_static_function(obj_name, prop_name)
                            {
                                return Ok(Expr::String("function".to_string()));
                            }
                            let is_process_object = ctx.lookup_local(obj_name).is_none()
                                && (obj_name == "process"
                                    || matches!(
                                        ctx.lookup_builtin_module_alias(obj_name),
                                        Some("process" | "node:process")
                                    )
                                    || matches!(
                                        ctx.lookup_native_module(obj_name),
                                        Some((
                                            "process"
                                                | "node:process"
                                                | "process.namespace"
                                                | "node:process.namespace"
                                                | "process.default"
                                                | "node:process.default",
                                            None
                                        ))
                                    ));
                            if is_process_object && prop_name == "sourceMapsEnabled" {
                                return Ok(Expr::String("boolean".to_string()));
                            }
                            // #1410 / #1400 / #1398 / #1409: `typeof
                            // process.ref` / `typeof process.unref` /
                            // `typeof process.setSourceMapsEnabled` /
                            // `typeof process.getBuiltinModule` /
                            // `typeof process.dlopen`. These methods
                            // lower to `Expr::Undefined` / no-ops when
                            // called; a bare member read still falls
                            // through to the generic process member path
                            // (returns 0 / "number" typeof), so fold to
                            // "function" here to match Node.
                            if is_process_object
                                && matches!(
                                    prop_name,
                                    "ref"
                                        | "unref"
                                        | "setSourceMapsEnabled"
                                        | "getBuiltinModule"
                                        | "dlopen"
                                        | "hasUncaughtExceptionCaptureCallback"
                                        | "setUncaughtExceptionCaptureCallback"
                                        | "loadEnvFile"
                                )
                            {
                                return Ok(Expr::String("function".to_string()));
                            }
                            if matches!(
                                ctx.lookup_native_instance(obj_name),
                                Some(("async_hooks", "AsyncHook"))
                            ) && matches!(prop_name, "enable" | "disable")
                            {
                                return Ok(Expr::String("function".to_string()));
                            }
                            if matches!(
                                ctx.lookup_native_instance(obj_name),
                                Some(("async_hooks", "AsyncResource"))
                            ) && matches!(
                                prop_name,
                                "asyncId"
                                    | "triggerAsyncId"
                                    | "runInAsyncScope"
                                    | "emitDestroy"
                                    | "bind"
                            ) {
                                return Ok(Expr::String("function".to_string()));
                            }
                            if matches!(
                                ctx.lookup_native_instance(obj_name),
                                Some(("events", "EventEmitterAsyncResource"))
                            ) && matches!(
                                prop_name,
                                "emitDestroy"
                                    | "on"
                                    | "addListener"
                                    | "once"
                                    | "prependListener"
                                    | "prependOnceListener"
                                    | "off"
                                    | "removeListener"
                                    | "removeAllListeners"
                                    | "emit"
                                    | "listenerCount"
                                    | "listeners"
                                    | "rawListeners"
                                    | "eventNames"
                                    | "setMaxListeners"
                                    | "getMaxListeners"
                            ) {
                                return Ok(Expr::String("function".to_string()));
                            }
                            // #1320: `typeof obs.observe` on a PerformanceObserver
                            // instance. A bare member read on a native-class
                            // instance lowers to a 0-arg NativeMethodCall (getter
                            // semantics), so `typeof` evaluated `observe()` and
                            // reported "undefined". These are methods, not
                            // getters — fold to "function" (the call form
                            // `obs.observe(...)` is unaffected).
                            if matches!(
                                ctx.lookup_native_instance(obj_name),
                                Some(("perf_hooks", _))
                            ) && matches!(prop_name, "observe" | "disconnect" | "takeRecords")
                            {
                                return Ok(Expr::String("function".to_string()));
                            }
                            // `readline.Interface` is a native handle whose
                            // value-read members lower as zero-arg native
                            // calls. For shape probes, fold `typeof` at the
                            // AST layer so we report Node's public surface
                            // without invoking those methods.
                            if matches!(
                                ctx.lookup_native_instance(obj_name),
                                Some(("readline", "Interface"))
                            ) {
                                if matches!(
                                    prop_name,
                                    "close"
                                        | "pause"
                                        | "resume"
                                        | "prompt"
                                        | "setPrompt"
                                        | "getPrompt"
                                        | "question"
                                        | "write"
                                        | "getCursorPos"
                                        | "on"
                                ) {
                                    return Ok(Expr::String("function".to_string()));
                                }
                                if prop_name == "line" {
                                    return Ok(Expr::String("string".to_string()));
                                }
                                if prop_name == "terminal" {
                                    return Ok(Expr::String("boolean".to_string()));
                                }
                            }
                            // #1698: `typeof req.json` on a Web Fetch Request /
                            // Response instance. The body methods are real
                            // functions in Node, but a bare LITERAL member read
                            // (`req.json`) takes the typed Web-Fetch codegen path,
                            // which returns the numeric handle (typeof "object")
                            // rather than routing to `dispatch_request_property`'s
                            // bound-method value (the COMPUTED `req[key]` form
                            // already does). Fold the literal-read typeof to
                            // "function" to match Node. The call form
                            // (`req.json()`) is unaffected.
                            if matches!(
                                ctx.lookup_native_instance(obj_name),
                                Some(("Request", "Request")) | Some(("fetch", "Response"))
                            ) && matches!(
                                prop_name,
                                "json"
                                    | "text"
                                    | "arrayBuffer"
                                    | "blob"
                                    | "bytes"
                                    | "formData"
                                    | "clone"
                            ) {
                                return Ok(Expr::String("function".to_string()));
                            }
                            // #677: `typeof Function.prototype` → "object".
                            // `Function.prototype` is the (immutable) prototype
                            // chain root for all functions; in Node typeof is
                            // "object". Other `Function.<X>` reads (`Function.name`,
                            // etc.) fall through to GlobalGet member-access,
                            // which today returns `undefined`.
                            if obj_name == "Function"
                                && prop_name == "prototype"
                                && ctx.lookup_local("Function").is_none()
                            {
                                return Ok(Expr::String("object".to_string()));
                            }
                        }
                    }
                    // `typeof "".methodName === "function"` — feature
                    // detection idiom. Generic PropertyGet on a string
                    // literal returns undefined in Perry today, so the
                    // typeof would be "undefined" and the test branch
                    // gets skipped. Fold to "function" when the property
                    // name is a known String.prototype method that the
                    // runtime actually dispatches.
                    if let (ast::Expr::Lit(ast::Lit::Str(_)), ast::MemberProp::Ident(prop_ident)) =
                        (member.obj.as_ref(), &member.prop)
                    {
                        let prop_name = prop_ident.sym.as_ref();
                        if is_known_string_prototype_method(prop_name) {
                            return Ok(Expr::String("function".to_string()));
                        }
                    }
                    // #1777: `typeof Array.prototype.slice` / `typeof [].slice`
                    // (and String/Number/Boolean prototypes). The method value
                    // read lowers to `undefined` today, so typeof was
                    // "undefined" — but these are real functions in Node and the
                    // `.call`/`.apply` dispatch is now wired (see
                    // `try_builtin_prototype_method_apply_call`). Fold to
                    // "function" for known prototype methods so feature
                    // detection (`typeof X.slice === "function"`) agrees.
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        let prop_name = prop_ident.sym.as_ref();
                        // `<Ctor>.prototype.<method>`
                        if let ast::Expr::Member(proto) = member.obj.as_ref() {
                            if let (ast::Expr::Ident(base), ast::MemberProp::Ident(proto_prop)) =
                                (proto.obj.as_ref(), &proto.prop)
                            {
                                let ctor = base.sym.as_ref();
                                if proto_prop.sym.as_ref() == "prototype"
                                    && ctx.lookup_local(ctor).is_none()
                                {
                                    // #2058: every built-in prototype inherits the
                                    // universal `Object.prototype` methods
                                    // (`isPrototypeOf`, `hasOwnProperty`,
                                    // `toString`, …), so `typeof
                                    // Object.prototype.isPrototypeOf` /
                                    // `typeof Number.prototype.hasOwnProperty` are
                                    // "function" in Node. Plus each ctor's own
                                    // prototype methods (and `Function.prototype`'s
                                    // `call`/`apply`/`bind`).
                                    let is_obj_proto = is_known_object_prototype_method(prop_name);
                                    let is_fn = match ctor {
                                        "Object" => is_obj_proto,
                                        "Function" => {
                                            is_obj_proto
                                                || matches!(prop_name, "call" | "apply" | "bind")
                                        }
                                        "Array" => {
                                            is_obj_proto
                                                || is_known_array_prototype_method(prop_name)
                                        }
                                        "String" => {
                                            is_obj_proto
                                                || is_known_string_prototype_method(prop_name)
                                        }
                                        // Number/Boolean prototypes: the handful of
                                        // ctor-specific methods plus the inherited
                                        // Object.prototype methods are all functions.
                                        "Number" => {
                                            is_obj_proto
                                                || matches!(
                                                    prop_name,
                                                    "toFixed" | "toPrecision" | "toExponential"
                                                )
                                        }
                                        "Boolean" => is_obj_proto,
                                        "TextEncoder" => {
                                            is_obj_proto
                                                || matches!(prop_name, "encode" | "encodeInto")
                                        }
                                        "TextDecoder" => is_obj_proto || prop_name == "decode",
                                        _ => false,
                                    };
                                    if is_fn {
                                        return Ok(Expr::String("function".to_string()));
                                    }
                                }
                            }
                        }
                        // `[].<method>` — array-literal prototype borrow.
                        if matches!(member.obj.as_ref(), ast::Expr::Array(_))
                            && is_known_array_prototype_method(prop_name)
                        {
                            return Ok(Expr::String("function".to_string()));
                        }
                        // #2143: `typeof Promise.resolve.bind` /
                        // `typeof Math.min.call` / `typeof JSON.parse.apply`.
                        // Built-in function values don't inherit
                        // `Function.prototype` in Perry's representation, so the
                        // chained `.bind`/`.call`/`.apply` read falls through to
                        // a numeric fallback (typeof "number"). Node treats
                        // these as real functions — fold here when the inner
                        // member names a known namespace static so feature
                        // detection (Test262 `propertyHelper.js`, the Promise
                        // tests cited in #793) sees callable values.
                        if matches!(prop_name, "bind" | "call" | "apply") {
                            if let ast::Expr::Member(inner) = member.obj.as_ref() {
                                if let (
                                    ast::Expr::Ident(inner_obj),
                                    ast::MemberProp::Ident(inner_prop),
                                ) = (inner.obj.as_ref(), &inner.prop)
                                {
                                    let inner_obj_name = inner_obj.sym.as_ref();
                                    let inner_prop_name = inner_prop.sym.as_ref();
                                    if ctx.lookup_local(inner_obj_name).is_none()
                                        && ctx.lookup_func(inner_obj_name).is_none()
                                        && is_known_namespace_static_function(
                                            inner_obj_name,
                                            inner_prop_name,
                                        )
                                    {
                                        return Ok(Expr::String("function".to_string()));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // Static `delete` folding only applies when no `with` environment
            // is active: inside `with(o) { delete x }`, `x` may resolve to a
            // configurable property of `o` and must be deleted at runtime
            // (Test262 11.4.1-4.a-6), so we leave those to the dynamic path.
            if unary.op == ast::UnaryOp::Delete && ctx.with_env_stack.is_empty() {
                // Peel parens: `delete (x)` deletes the inner reference.
                let mut bare = unary.arg.as_ref();
                while let ast::Expr::Paren(p) = bare {
                    bare = p.expr.as_ref();
                }
                if let ast::Expr::Member(member) = bare {
                    if let (ast::Expr::Ident(obj), ast::MemberProp::Ident(prop)) =
                        (member.obj.as_ref(), &member.prop)
                    {
                        let obj_name = obj.sym.as_ref();
                        let prop_name = prop.sym.as_ref();
                        let is_global = ctx.lookup_local(obj_name).is_none()
                            && ctx.lookup_func(obj_name).is_none();
                        if is_global
                            && obj_name == "Number"
                            && matches!(
                                prop_name,
                                "NaN"
                                    | "POSITIVE_INFINITY"
                                    | "NEGATIVE_INFINITY"
                                    | "MAX_VALUE"
                                    | "MIN_VALUE"
                                    | "EPSILON"
                                    | "MAX_SAFE_INTEGER"
                                    | "MIN_SAFE_INTEGER"
                            )
                        {
                            return Ok(Expr::Bool(false));
                        }
                        // `Math`'s numeric constants are non-configurable, so
                        // `delete Math.PI` is `false` (Math's *methods* stay
                        // configurable, hence `delete Math.abs` is `true` and
                        // is left to the generic path). Test262 S8.12.7_A1.
                        if is_global
                            && obj_name == "Math"
                            && matches!(
                                prop_name,
                                "E" | "LN10"
                                    | "LN2"
                                    | "LOG10E"
                                    | "LOG2E"
                                    | "PI"
                                    | "SQRT1_2"
                                    | "SQRT2"
                            )
                        {
                            return Ok(Expr::Bool(false));
                        }
                    }
                }
                // `delete <BindingIdentifier>` — deleting a reference to a
                // resolvable binding (var / let / const / function / param /
                // class / import) is non-configurable, so it evaluates to
                // `false` without removing anything (spec 13.5.1.2). The bare
                // globals `undefined` / `NaN` / `Infinity` are likewise
                // non-configurable global properties → `false`. Any other
                // unresolvable bare identifier (an implicit global from
                // `x = 1`, or a configurable global builtin) is `true` in
                // sloppy mode — lowering it as a literal avoids the spurious
                // ReferenceError the operand-evaluation path would throw.
                if let ast::Expr::Ident(id) = bare {
                    let name = id.sym.as_ref();
                    // Bare globals that are non-configurable → false.
                    if name == "arguments" || matches!(name, "undefined" | "NaN" | "Infinity") {
                        return Ok(Expr::Bool(false));
                    }
                    if let Some(lid) = ctx.lookup_local(name) {
                        // `x = 1` with no declaration creates a *configurable*
                        // global property (`delete x` → true); a real
                        // var/let/const/param binding is non-configurable
                        // (→ false). Distinguish via the implicit-global set.
                        if ctx.sloppy_implicit_global_ids.contains(&lid) {
                            return Ok(Expr::Bool(true));
                        }
                        // At module top level a bare `x = 1` becomes an ordinary
                        // module-level local indistinguishable from `var x = 1`
                        // (the implicit-global path isn't taken there), so we
                        // can't statically tell a non-configurable `var`/`let`
                        // binding from a configurable implicit global — defer to
                        // the runtime delete (Test262 S11.4.1_A3.2_T1). Inside a
                        // function, an implicit global *does* go through the
                        // sloppy-global set, so a plain local here is a genuine
                        // binding → false.
                        if !ctx.module_level_ids.contains(&lid) {
                            return Ok(Expr::Bool(false));
                        }
                        // module-level local: fall through to the dynamic path.
                    } else if ctx.lookup_func(name).is_some()
                        || ctx.lookup_class(name).is_some()
                        || ctx.lookup_imported_func(name).is_some()
                    {
                        return Ok(Expr::Bool(false));
                    } else {
                        // Truly unresolvable bare identifier (no binding, no
                        // known global) → `true` in sloppy mode; lowering it as
                        // a literal avoids a spurious ReferenceError from the
                        // operand-evaluation path.
                        return Ok(Expr::Bool(true));
                    }
                }
            }
            let operand = Box::new(lower_expr(ctx, &unary.arg)?);
            match unary.op {
                ast::UnaryOp::Minus => {
                    // Fold -Number into Number(-val) to simplify codegen
                    // (e.g., array literals with negative numbers avoid Unary wrapper)
                    if let Expr::Number(val) = *operand {
                        Ok(Expr::Number(-val))
                    } else if let Expr::Integer(val) = *operand {
                        // Special case: -0 must be preserved as -0.0 (negative zero)
                        // because integers collapse +0 and -0 into the same bit pattern.
                        // JS distinguishes these in `console.log`, `Object.is`, and
                        // `1/x` — so fold to Number(-0.0) instead of Integer(0).
                        if val == 0 {
                            Ok(Expr::Number(-0.0))
                        } else {
                            Ok(Expr::Integer(-val))
                        }
                    } else {
                        Ok(Expr::Unary {
                            op: UnaryOp::Neg,
                            operand,
                        })
                    }
                }
                ast::UnaryOp::Plus => Ok(Expr::Unary {
                    op: UnaryOp::Pos,
                    operand,
                }),
                ast::UnaryOp::Bang => Ok(Expr::Unary {
                    op: UnaryOp::Not,
                    operand,
                }),
                ast::UnaryOp::Tilde => Ok(Expr::Unary {
                    op: UnaryOp::BitNot,
                    operand,
                }),
                ast::UnaryOp::TypeOf => {
                    // Fast path: known Symbol-producing expressions resolve to "symbol"
                    // at compile time (avoids needing runtime js_value_typeof to
                    // recognize the SymbolHeader magic).
                    if matches!(&*operand, Expr::SymbolNew(_) | Expr::SymbolFor(_)) {
                        return Ok(Expr::String("symbol".to_string()));
                    }
                    Ok(Expr::TypeOf(operand))
                }
                ast::UnaryOp::Delete => {
                    // `delete super.prop` / `delete super[expr]` is always a
                    // ReferenceError (the operand is a SuperProperty reference,
                    // which `delete` rejects). Peel parens to catch
                    // `delete (super.x)`. Args of a computed super key are
                    // evaluated first for side effects.
                    let mut del_arg = unary.arg.as_ref();
                    while let ast::Expr::Paren(p) = del_arg {
                        del_arg = p.expr.as_ref();
                    }
                    if let ast::Expr::SuperProp(super_prop) = del_arg {
                        let throw =
                            throw_reference_error_expr("js_throw_reference_error_super_delete");
                        if let ast::SuperProp::Computed(computed) = &super_prop.prop {
                            let key = lower_expr(ctx, computed.expr.as_ref())?;
                            return Ok(Expr::Sequence(vec![key, throw]));
                        }
                        return Ok(throw);
                    }
                    // Proxy delete: rewrite `delete proxy.key` as ProxyDelete.
                    if let Expr::ProxyGet { proxy, key } = &*operand {
                        return Ok(Expr::ProxyDelete {
                            proxy: proxy.clone(),
                            key: key.clone(),
                        });
                    }
                    Ok(Expr::Delete(operand))
                }
                ast::UnaryOp::Void => Ok(Expr::Void(operand)),
                // #853: `ast::UnaryOp` is `#[non_exhaustive]` upstream — keep
                // this catch-all as a forward-compat safety net.
                #[allow(unreachable_patterns)]
                _ => Err(anyhow!("Unsupported unary operator: {:?}", unary.op)),
            }
        }
        ast::Expr::Call(call) => expr_call::lower_call(ctx, call),
        ast::Expr::Member(member) => expr_member::lower_member(ctx, member),
        ast::Expr::Paren(paren) => lower_expr(ctx, &paren.expr),
        ast::Expr::Assign(assign) => expr_assign::lower_assign(ctx, assign),
        ast::Expr::Cond(cond) => expr_misc::lower_cond(ctx, cond),
        ast::Expr::Array(array) => {
            // Check if any elements need the spread-aware representation.
            let has_spread = array
                .elems
                .iter()
                .filter_map(|elem| elem.as_ref())
                .any(|elem| elem.spread.is_some());
            let has_hole = array.elems.iter().any(|elem| elem.is_none());

            if has_spread || has_hole {
                // Use ArraySpread for arrays with spread elements or elisions.
                // Elisions must remain holes, not explicit undefined values:
                // own-property checks and iteration observe the difference.
                let elements = array
                    .elems
                    .iter()
                    .map(|elem| {
                        let Some(elem) = elem.as_ref() else {
                            return Ok(ArrayElement::Hole);
                        };
                        let expr = lower_expr(ctx, &elem.expr)?;
                        if elem.spread.is_some() {
                            if is_generator_call_expr(ctx, &expr) {
                                Ok(ArrayElement::Spread(Expr::IteratorToArray(Box::new(expr))))
                            } else {
                                Ok(ArrayElement::Spread(expr))
                            }
                        } else {
                            Ok(ArrayElement::Expr(expr))
                        }
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(Expr::ArraySpread(elements))
            } else {
                let elements = array
                    .elems
                    .iter()
                    .map(|elem| lower_expr(ctx, &elem.as_ref().unwrap().expr))
                    .collect::<Result<Vec<_>>>()?;
                Ok(Expr::Array(elements))
            }
        }
        ast::Expr::Object(obj) => expr_object::lower_object(ctx, obj),
        ast::Expr::This(_) => {
            // Module TOP-LEVEL `this` is Node-CJS `module.exports` — a fresh
            // plain object, not `globalThis` (the oracle runs assembled test
            // files as CommonJS). Function/class/with bodies keep dynamic
            // `Expr::This` semantics, handled by codegen's ThisContext.
            if ctx.scope_depth == 0
                && ctx.current_class.is_none()
                && ctx.with_env_stack.is_empty()
                && !ctx.is_external_module
            {
                return Ok(Expr::ModuleTopThis);
            }
            Ok(Expr::This)
        }
        ast::Expr::New(new_expr) => expr_new::lower_new(ctx, new_expr),
        ast::Expr::Arrow(arrow) => expr_function::lower_arrow(ctx, arrow),
        ast::Expr::Fn(fn_expr) => expr_function::lower_fn_expr(ctx, fn_expr),
        ast::Expr::Await(await_expr) => expr_misc::lower_await(ctx, await_expr),
        ast::Expr::SuperProp(super_prop) => expr_misc::lower_super_prop(ctx, super_prop),
        ast::Expr::Update(update) => expr_misc::lower_update(ctx, update),
        ast::Expr::Tpl(tpl) => expr_misc::lower_tpl(ctx, tpl),
        ast::Expr::OptChain(opt_chain) => {
            // Optional chaining: obj?.prop or obj?.[index] or obj?.method()
            // Convert to: obj == null ? undefined : obj.prop
            match &*opt_chain.base {
                ast::OptChainBase::Member(member) => {
                    // Issue #449: `new.target?.<prop>` folds to a literal at
                    // lowering time — same shape as the direct
                    // `new.target.<prop>` fold in `expr_member::lower_member`,
                    // applied here BEFORE `lower_expr(&member.obj)` would
                    // otherwise route MetaProp(NewTarget) through the
                    // broken Object-literal synthesis path. Inside a
                    // constructor `new.target` is non-null/non-undefined,
                    // so the optional chain just resolves the property;
                    // outside a constructor it's undefined and the chain
                    // short-circuits.
                    if let ast::Expr::MetaProp(mp) = member.obj.as_ref() {
                        if matches!(mp.kind, ast::MetaPropKind::NewTarget) {
                            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                                let prop_name = prop_ident.sym.as_ref();
                                if let Some(class_name) = ctx.in_constructor_class.clone() {
                                    return Ok(match prop_name {
                                        "name" => Expr::String(class_name),
                                        _ => Expr::Undefined,
                                    });
                                }
                                return Ok(Expr::Undefined);
                            }
                        }
                    }
                    // obj?.prop -> obj == null ? undefined : obj.prop
                    let obj_expr = lower_expr(ctx, &member.obj)?;

                    // Get the property access
                    let prop_expr = match &member.prop {
                        ast::MemberProp::Ident(ident) => {
                            let prop_name = ident.sym.to_string();
                            // RegExp exec/match `.index` / `.groups` / `.input`
                            // are real own properties on the result array
                            // (regex.rs), so they resolve as a generic
                            // PropertyGet — no thread-local fold. This keeps a
                            // stored result correct after an intervening match
                            // on another regex.
                            Expr::PropertyGet {
                                object: Box::new(obj_expr.clone()),
                                property: prop_name,
                            }
                        }
                        ast::MemberProp::Computed(comp) => {
                            let index = lower_expr(ctx, &comp.expr)?;
                            Expr::IndexGet {
                                object: Box::new(obj_expr.clone()),
                                index: Box::new(index),
                            }
                        }
                        ast::MemberProp::PrivateName(private) => {
                            let property = format!("#{}", private.name);
                            let object = expr_member::wrap_private_guard(
                                ctx,
                                Box::new(obj_expr.clone()),
                                &property,
                                expr_member::PRIV_OP_READ,
                            );
                            Expr::PropertyGet { object, property }
                        }
                    };

                    // Issue #388: optional chaining short-circuits on
                    // null OR undefined per spec. Use `LooseEq` so the
                    // comparison `obj == null` matches both — strict
                    // `===` only matches null, leaving undefined to
                    // fall through and dereference (returning
                    // `[object Object]` for Map.get's missing value).
                    Ok(Expr::Conditional {
                        condition: Box::new(Expr::Compare {
                            op: CompareOp::LooseEq,
                            left: Box::new(obj_expr),
                            right: Box::new(Expr::Null),
                        }),
                        then_expr: Box::new(Expr::Undefined),
                        else_expr: Box::new(prop_expr),
                    })
                }
                ast::OptChainBase::Call(call) => {
                    // OptChain(Call) is `<expr>?.(args)` — the `?.` is between the
                    // callee and the call parens (e.g. `obj.method?.(args)`), NOT
                    // `obj?.method(args)` (which SWC parses as Call(OptChain(Member))
                    // and is handled via the regular Call lowering path).
                    //
                    // So the short-circuit must check the *function value* (the
                    // callee), not the receiver. Issue #830: previously this
                    // checked `obj == null`, which crashed when `obj.method` was
                    // undefined while `obj` itself was a valid object.
                    let callee = &call.callee;

                    // Check for spread arguments
                    let has_spread = call.args.iter().any(|arg| arg.spread.is_some());

                    let args = call
                        .args
                        .iter()
                        .map(|arg| lower_expr(ctx, &arg.expr))
                        .collect::<Result<Vec<_>>>()?;

                    // Lower callee as plain MemberExpr, unwrapping inner OptChain.
                    // SWC may wrap the callee member access in an OptChain too.
                    // We must NOT re-lower via lower_expr which would nest Conditionals.
                    //
                    // `callee_from_chain` records the `foo?.bar?.(args)` shape: the
                    // callee is itself an optional chain, so `check_expr` is the
                    // *receiver* (`foo`) rather than the function value. In that
                    // case the receiver short-circuit alone is not enough — the
                    // function value (`foo.bar`) must ALSO be null-checked before
                    // the call, or an `undefined` property is invoked and throws
                    // "X is not a function" (issue #4699: zod `safeParse`'s
                    // `iss.inst?._zod.def?.error?.(iss)` error-map probe).
                    let mut callee_from_chain = false;
                    let (check_expr, callee_expr) = {
                        let mut lower_member_flat =
                            |member: &ast::MemberExpr| -> Result<(Expr, Expr)> {
                                let obj = lower_expr(ctx, &member.obj)?;
                                let prop = match &member.prop {
                                    ast::MemberProp::Ident(id) => Expr::PropertyGet {
                                        object: Box::new(obj.clone()),
                                        property: id.sym.to_string(),
                                    },
                                    ast::MemberProp::Computed(c) => {
                                        let idx = lower_expr(ctx, &c.expr)?;
                                        Expr::IndexGet {
                                            object: Box::new(obj.clone()),
                                            index: Box::new(idx),
                                        }
                                    }
                                    ast::MemberProp::PrivateName(private) => {
                                        let property = format!("#{}", private.name);
                                        let guarded = expr_member::wrap_private_guard(
                                            ctx,
                                            Box::new(obj.clone()),
                                            &property,
                                            expr_member::PRIV_OP_READ,
                                        );
                                        Expr::PropertyGet {
                                            object: guarded,
                                            property,
                                        }
                                    }
                                };
                                Ok((obj, prop))
                            };
                        match &**callee {
                            // Simple `obj.method?.(args)`: check the function value
                            // (prop), call the function (prop) — codegen still sees
                            // a PropertyGet callee so `this` binds to obj.
                            ast::Expr::Member(m) => {
                                let (_obj, prop) = lower_member_flat(m)?;
                                (prop.clone(), prop)
                            }
                            ast::Expr::OptChain(inner) => match &*inner.base {
                                // The callee is itself an optional chain. Two
                                // distinct shapes land here, told apart by whether
                                // THIS chain link's call is optional
                                // (`opt_chain.optional`, the `?.(` token):
                                //
                                //  • `foo?.bar?.(args)` (optional call): check the
                                //    receiver (foo) so the inner `?.` short-circuit
                                //    works, AND flag that the function value
                                //    (foo.bar) needs its own null-check before the
                                //    call (#4699 — an `undefined` property must
                                //    short-circuit, not throw "X is not a function").
                                //
                                //  • `foo?.bar(args)` (non-optional call, only the
                                //    member is optional): this is an ordinary method
                                //    call guarded by the receiver. It must NOT get a
                                //    function-value guard — `s?.at(-1)` reads `s.at`
                                //    as a bare PropertyGet, which is `undefined` for
                                //    builtin (string/array) methods that only resolve
                                //    through the call path, so the guard would wrongly
                                //    short-circuit the whole call (#4814). Leaving
                                //    `callee_from_chain` false yields the plain
                                //    `recv == null ? undefined : recv.method(args)`,
                                //    and codegen binds `this` from the PropertyGet
                                //    callee + dispatches the builtin normally.
                                ast::OptChainBase::Member(m) => {
                                    callee_from_chain = opt_chain.optional;
                                    lower_member_flat(m)?
                                }
                                _ => {
                                    let ce = lower_expr(ctx, callee)?;
                                    (ce.clone(), ce)
                                }
                            },
                            _ => {
                                let ce = lower_expr(ctx, callee)?;
                                (ce.clone(), ce)
                            }
                        }
                    };

                    // If check_expr is already a Conditional from an inner optional chain,
                    // nest the outer call inside its else branch instead of creating another Conditional.
                    // This avoids duplicating side-effecting expressions (like ArrayShift/ArrayPop).
                    if let Expr::Conditional {
                        condition: inner_cond,
                        then_expr: inner_then,
                        else_expr: inner_else,
                    } = check_expr
                    {
                        // Build the callee with inner_else as the object (not the full Conditional)
                        let fixed_callee = match callee_expr {
                            Expr::PropertyGet { property, .. } => Expr::PropertyGet {
                                object: inner_else,
                                property,
                            },
                            Expr::IndexGet { index, .. } => Expr::IndexGet {
                                object: inner_else,
                                index,
                            },
                            other => other,
                        };
                        let outer_call = Expr::Call {
                            callee: Box::new(fixed_callee.clone()),
                            args,
                            type_args: Vec::new(),
                        };
                        // For `foo?.bar?.(args)` the function value (`bar` on the
                        // un-short-circuited receiver) must itself be null-checked
                        // before calling — otherwise an `undefined` property is
                        // invoked and throws "X is not a function" (#4699).
                        let else_expr: Box<Expr> = if callee_from_chain {
                            Box::new(Expr::Conditional {
                                condition: Box::new(Expr::Compare {
                                    op: CompareOp::LooseEq,
                                    left: Box::new(fixed_callee),
                                    right: Box::new(Expr::Null),
                                }),
                                then_expr: Box::new(Expr::Undefined),
                                else_expr: Box::new(outer_call),
                            })
                        } else {
                            Box::new(outer_call)
                        };
                        return Ok(Expr::Conditional {
                            condition: inner_cond,
                            then_expr: inner_then,
                            else_expr,
                        });
                    }

                    // Keep the function value for the `foo?.bar?.(args)` guard
                    // (see callee_from_chain) before it is moved into the call.
                    let func_value_for_guard = if callee_from_chain {
                        Some(callee_expr.clone())
                    } else {
                        None
                    };

                    // Build the call expression
                    let call_expr = if has_spread {
                        let spread_args: Vec<CallArg> = call
                            .args
                            .iter()
                            .zip(args.iter())
                            .map(|(ast_arg, lowered)| {
                                if ast_arg.spread.is_some() {
                                    CallArg::Spread(lowered.clone())
                                } else {
                                    CallArg::Expr(lowered.clone())
                                }
                            })
                            .collect();
                        Expr::CallSpread {
                            callee: Box::new(callee_expr),
                            args: spread_args,
                            type_args: Vec::new(),
                        }
                    } else {
                        // Try to fold known array methods (`.map`/`.filter`/etc.)
                        // into their dedicated HIR variants here, since the regular
                        // `lower_expr` Call array fast-path is on the AST CallExpr
                        // path and never sees the synthetic Expr::Call we build
                        // for `obj?.method(args)`.
                        try_fold_array_method_call(Expr::Call {
                            callee: Box::new(callee_expr),
                            args,
                            type_args: Vec::new(),
                        })
                    };

                    // For `foo?.bar?.(args)` the receiver check below guards `foo`,
                    // but the function value `foo.bar` must ALSO be null-checked
                    // before the call — otherwise an `undefined` property is
                    // invoked and throws "X is not a function" (#4699).
                    let else_expr: Box<Expr> = match func_value_for_guard {
                        Some(func_value) => Box::new(Expr::Conditional {
                            condition: Box::new(Expr::Compare {
                                op: CompareOp::LooseEq,
                                left: Box::new(func_value),
                                right: Box::new(Expr::Null),
                            }),
                            then_expr: Box::new(Expr::Undefined),
                            else_expr: Box::new(call_expr),
                        }),
                        None => Box::new(call_expr),
                    };

                    // Issue #388: optional chaining short-circuits on
                    // null OR undefined per spec. Use `LooseEq` so the
                    // comparison `check_expr == null` matches both —
                    // strict `===` only matches null, leaving
                    // undefined to fall through and produce
                    // `[object Object]` (or worse) when the receiver
                    // is `Map.get(missing)` etc.
                    Ok(Expr::Conditional {
                        condition: Box::new(Expr::Compare {
                            op: CompareOp::LooseEq,
                            left: Box::new(check_expr),
                            right: Box::new(Expr::Null),
                        }),
                        then_expr: Box::new(Expr::Undefined),
                        else_expr,
                    })
                }
            }
        }
        ast::Expr::TsAs(ts_as) => {
            // TypeScript 'as' type assertion - at runtime, just evaluate the expression
            // The type assertion is compile-time only
            lower_expr_with_json_parse_type_hint(ctx, &ts_as.expr, &ts_as.type_ann)
        }
        ast::Expr::TsNonNull(ts_non_null) => {
            // TypeScript non-null assertion (value!) - at runtime, just the expression
            lower_expr(ctx, &ts_non_null.expr)
        }
        ast::Expr::TsTypeAssertion(ts_assertion) => {
            // TypeScript angle-bracket type assertion (<Type>value) - same as 'as', compile-time only
            lower_expr_with_json_parse_type_hint(ctx, &ts_assertion.expr, &ts_assertion.type_ann)
        }
        ast::Expr::TsConstAssertion(ts_const) => {
            // TypeScript 'as const' assertion - at runtime, just evaluate the expression
            // The const assertion only affects type inference, not runtime behavior
            lower_expr(ctx, &ts_const.expr)
        }
        ast::Expr::TsSatisfies(ts_satisfies) => {
            // TypeScript 'satisfies' operator - compile-time type check only
            lower_expr(ctx, &ts_satisfies.expr)
        }
        ast::Expr::TsInstantiation(ts_inst) => {
            // TypeScript generic instantiation (func<Type>) - at runtime, just the expression
            lower_expr(ctx, &ts_inst.expr)
        }
        ast::Expr::Seq(seq) => expr_misc::lower_seq(ctx, seq),
        ast::Expr::MetaProp(meta_prop) => expr_misc::lower_meta_prop(ctx, meta_prop),
        ast::Expr::Yield(y) => expr_misc::lower_yield(ctx, y),
        ast::Expr::TaggedTpl(tagged) => {
            // Tagged template literals: tag`Hello ${name},${42}!`
            // Two cases:
            //  (a) String.raw — kept as a fast-path string concatenation that
            //      preserves backslashes literally (no escape processing).
            //  (b) Any other tag function — desugar to a regular function call:
            //      tag(["Hello ", ",", "!"], name, 42)
            //      i.e. first arg is the array of cooked string literal parts,
            //      followed by each interpolated value as its own argument.
            //      The matches the JS spec for `tag` callbacks (sans `.raw`).
            let is_string_raw = match &*tagged.tag {
                ast::Expr::Member(member) => {
                    let obj_is_string = match &member.obj.as_ref() {
                        ast::Expr::Ident(id) => id.sym.as_ref() == "String",
                        _ => false,
                    };
                    let prop_is_raw = match &member.prop {
                        ast::MemberProp::Ident(id) => id.sym.as_ref() == "raw",
                        _ => false,
                    };
                    obj_is_string && prop_is_raw
                }
                _ => false,
            };

            let tpl = &*tagged.tpl;
            if tpl.quasis.is_empty() {
                return Ok(Expr::String(String::new()));
            }

            if is_string_raw {
                // Fast path: build string via direct concatenation using `raw` text
                let first_raw = tpl.quasis.first().map(|q| q.raw.as_ref()).unwrap_or("");
                let mut result = Expr::String(first_raw.to_string());

                for (i, expr) in tpl.exprs.iter().enumerate() {
                    let lowered = lower_expr(ctx, expr)?;
                    result = Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(result),
                        right: Box::new(lowered),
                    };

                    if let Some(quasi) = tpl.quasis.get(i + 1) {
                        let quasi_str: &str = quasi.raw.as_ref();
                        if !quasi_str.is_empty() {
                            result = Expr::Binary {
                                op: BinaryOp::Add,
                                left: Box::new(result),
                                right: Box::new(Expr::String(quasi_str.to_string())),
                            };
                        }
                    }
                }

                return Ok(result);
            }

            // General case: desugar to `tag(stringsArray, ...exprs)`. The
            // strings array carries the cooked text (escapes processed) AS
            // the array elements AND the raw text (escapes preserved) via
            // a thread-local side table populated at the call site —
            // `TaggedTemplateStrings` codegen emits both arrays, then asks
            // the runtime for the cached frozen template object so
            // `strings.raw` reads can resolve via the matching
            // `Expr::TemplateRaw` fold below.
            let cooked_strings: Vec<Expr> = tpl
                .quasis
                .iter()
                .map(|q| {
                    let cooked_owned: Option<String> = q
                        .cooked
                        .as_ref()
                        .and_then(|c| c.as_str().map(|s| s.to_string()));
                    let s = cooked_owned.unwrap_or_else(|| q.raw.as_ref().to_string());
                    Expr::String(s)
                })
                .collect();
            let raw_strings: Vec<String> = tpl
                .quasis
                .iter()
                .map(|q| q.raw.as_ref().to_string())
                .collect();
            let strings_array = Expr::TaggedTemplateStrings {
                site_id: ctx.fresh_tagged_template_site_id(),
                cooked: cooked_strings,
                raw: raw_strings,
            };

            let mut call_args: Vec<Expr> = Vec::with_capacity(tpl.exprs.len() + 1);
            call_args.push(strings_array);
            for e in &tpl.exprs {
                call_args.push(lower_expr(ctx, e)?);
            }

            let callee = lower_expr(ctx, &tagged.tag)?;
            Ok(Expr::Call {
                callee: Box::new(callee),
                args: call_args,
                type_args: vec![],
            })
        }
        // Class expression used as a value (not in `new` context) —
        // refs #740. JS semantics: a class expression evaluates to the
        // class constructor itself. Previously we emitted an empty `new`
        // here, which bound the local to a zero-arg instance instead of
        // the class — so `const C = class { ... }; new C(args)` ran the
        // ctor with no args, and `O.Inner` inside an object literal held
        // a stillborn instance instead of a constructor. Lower to a
        // `ClassRef` so the constructor identity survives the value path
        // and `new` site rerouting (via `local_class_aliases`) picks it
        // back up.
        ast::Expr::Class(class_expr) => {
            let ident_name = class_expr.ident.as_ref().map(|i| i.sym.to_string());
            let synthetic_name = ident_name.unwrap_or_else(|| {
                if !anonymous_class_has_static_name_member(&class_expr.class) {
                    if let Some(name) = ctx.assignment_inferred_name.as_ref() {
                        if !name.is_empty() {
                            return name.clone();
                        }
                    }
                }
                format!("__anon_class_{}", ctx.fresh_class())
            });
            let class = lower_class_from_ast(ctx, &class_expr.class, &synthetic_name, false)?;
            // Mixin factories like `function WithA(B) { return class extends B {} }`
            // produce a class whose super is the function-parameter `B` — a
            // runtime value, not a statically-known class. The class-decl arm
            // at the top of this file only pushes a `RegisterClassParentDynamic`
            // statement for top-level class declarations; an anonymous class
            // expression inside a function body never has that side effect
            // fire, so `new (class extends WithA(Base) {})().baseMethod()`
            // walks subclass → inner factory class and stops at the unwired
            // grandparent edge (TypeError on the inherited method). Sequence
            // the dynamic-parent registration in front of the ClassRef so the
            // edge is wired every time the factory function executes; the
            // Sequence yields its last element, so the value remains the
            // ClassRef the call site expects.
            let parent_expr = class.extends_expr.clone();
            // Issue #894: collect computed-Symbol-key static fields so
            // codegen emits a `RegisterClassStaticSymbol` registration
            // sequenced in front of the ClassRef. Without this, the
            // registration happens at module init via
            // `init_static_fields_late` — but the values referenced by
            // the key/init may not be valid yet (the factory hasn't been
            // called, so any function-local captures are zero) or the
            // class lookup may happen BEFORE module init's late phase
            // (within the same module's top-level expressions). Effect's
            // `make()` factory's `static [TypeId] = variance` is the
            // canonical case: `isSchema(C)` was called from Schema.ts's
            // own top-level `class extends transform(...)` chains, which
            // run before the module's `init_static_fields_late`.
            let static_symbol_registrations: Vec<(Expr, Expr)> = class
                .static_fields
                .iter()
                .filter_map(|sf| match (sf.key_expr.as_ref(), sf.init.as_ref()) {
                    (Some(k), Some(v)) => Some((k.clone(), v.clone())),
                    _ => None,
                })
                .collect();
            // Issue #1772: regular-named static fields with an initializer
            // (`static ast = ast`). #894 only handled the Symbol-key case;
            // these need the same per-evaluation treatment, otherwise a class
            // expression returned from a factory (effect's `make`) shares one
            // template class and `.ast` is undefined/clobbered.
            let named_statics: Vec<(String, Expr)> = class
                .static_fields
                .iter()
                .filter_map(|sf| match (sf.key_expr.as_ref(), sf.init.as_ref()) {
                    (None, Some(v)) => Some((sf.name.clone(), v.clone())),
                    _ => None,
                })
                .collect();
            let computed_member_registrations: Vec<Expr> = class
                .computed_members
                .iter()
                .map(|member| class_computed_member_registration_expr(&synthetic_name, member))
                .collect();
            let captured_args: Vec<Expr> = ctx
                .lookup_class_captures(&synthetic_name)
                .map(|ids| ids.iter().map(|id| Expr::LocalGet(*id)).collect())
                .unwrap_or_default();
            // Static block synthetic-method names (`__perry_static_init_N`), in
            // source order — emitted as inline `StaticMethodCall`s on the
            // shared-template path so blocks run at class-evaluation time (the
            // same treatment the class-declaration path gives them).
            let static_block_names: Vec<String> = class
                .static_methods
                .iter()
                .filter(|m| m.name.starts_with("__perry_static_init_"))
                .map(|m| m.name.clone())
                .collect();
            ctx.pending_classes.push(class);
            // #1772: a class EXPRESSION that carries per-evaluation static
            // fields and is NOT a mixin (`class extends <expr>`) lowers to a
            // fresh heap class object per evaluation (`ClassExprFresh`), so
            // `make(a) !== make(b)` and each holds its own statics as own
            // properties. Mixins and class expressions without statics/captures
            // keep the historical (shared-template) path.
            // A class expression evaluated at module top level runs exactly
            // once, so it needs no per-evaluation freshness — route it through
            // the shared-template `ClassRef` path (identical to a class
            // declaration), where static field/element initializers run via
            // `init_static_fields_late` and a static method's `this` resolves
            // to the class-ref. The `ClassExprFresh` path is reserved for class
            // expressions inside a function body (factories like effect's
            // `make()`), which produce a distinct class object per call.
            let at_module_top = ctx.scope_depth == 0 && ctx.inside_block_scope == 0;
            if !at_module_top
                && parent_expr.is_none()
                && (!named_statics.is_empty()
                    || !static_symbol_registrations.is_empty()
                    || !captured_args.is_empty())
            {
                // #1787: snapshot the class's captured outer-scope values so a
                // later `new <classObjectValue>()` can run the instance-field
                // initializers / constructor body with the right environment.
                // `synthesize_class_captures` (run during `lower_class_from_ast`
                // above) appended one `__perry_cap_<id>` constructor param per
                // captured outer id, in `captures_vec` order — read them back in
                // that same order as `LocalGet(outer_id)`, evaluated here where
                // the captures are still live.
                let fresh_expr = Expr::ClassExprFresh {
                    template: synthetic_name,
                    named_statics,
                    symbol_statics: static_symbol_registrations,
                    captured_args,
                };
                if computed_member_registrations.is_empty() {
                    return Ok(fresh_expr);
                }
                let mut seq = computed_member_registrations;
                seq.push(fresh_expr);
                return Ok(Expr::Sequence(seq));
            }
            let mut seq: Vec<Expr> = Vec::new();
            if let Some(p) = parent_expr {
                seq.push(Expr::RegisterClassParentDynamic {
                    class_name: synthetic_name.clone(),
                    parent_expr: p,
                });
            }
            seq.extend(computed_member_registrations);
            for (k, v) in static_symbol_registrations {
                seq.push(Expr::RegisterClassStaticSymbol {
                    class_name: synthetic_name.clone(),
                    key_expr: Box::new(k),
                    value_expr: Box::new(v),
                });
            }
            // Inline the named static field/element initializers at the point
            // the class expression evaluates (source order), mirroring the
            // class-declaration path. Without this the shared-template path
            // relied solely on the late `init_static_fields_late` pass, which
            // runs AFTER the surrounding top-level statements — so a read like
            // `C.x` immediately after `var C = class { static x = 1 }` saw the
            // uninitialized (0.0) slot. (Private statics carry a `#`-prefixed
            // name and flow through the same StaticFieldSet path.)
            for (name, v) in named_statics {
                seq.push(Expr::StaticFieldSet {
                    class_name: synthetic_name.clone(),
                    field_name: name,
                    value: Box::new(v),
                });
            }
            // Static blocks run right after the static-field initializers, in
            // source order, with the class as `this`.
            for block_name in static_block_names {
                seq.push(Expr::StaticMethodCall {
                    class_name: synthetic_name.clone(),
                    method_name: block_name,
                    args: Vec::new(),
                });
            }
            if seq.is_empty() {
                Ok(Expr::ClassRef(synthetic_name))
            } else {
                seq.push(Expr::ClassRef(synthetic_name));
                Ok(Expr::Sequence(seq))
            }
        }
        ast::Expr::JSXElement(jsx) => lower_jsx_element(ctx, jsx),
        ast::Expr::JSXFragment(jsx) => lower_jsx_fragment(ctx, jsx),
        _ => Err(anyhow!("Unsupported expression type: {:?}", expr)),
    }
}

fn lower_expr_with_json_parse_type_hint(
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

fn typed_parse_codegen_supports(ty: &Type) -> bool {
    let elem = match ty {
        Type::Array(inner) => inner.as_ref(),
        Type::Generic { base, type_args } if base == "Array" && type_args.len() == 1 => {
            &type_args[0]
        }
        _ => return false,
    };

    matches!(elem, Type::Object(obj) if !obj.properties.is_empty())
}

/// If `call` matches `Text(\`...${state.value}...\`)` with at least one State
/// interpolation, desugar into an auto-reactive binding. Returns `Ok(None)`
/// for anything else so the generic Call lowering runs.
///
/// The promise (docs/src/ui/state.md): *"Perry detects `state.value` reads
/// inside template literals and creates reactive bindings."* Prior to this,
/// the detection existed nowhere and `count.set(...)` didn't update the
/// rendered label on any platform — most visibly on web/wasm (issue #104)
/// where users ran the counter example and saw static text.
///
/// Generated HIR shape:
/// ```text
/// Sequence([
///   LocalSet(__h, Text(initial_concat)),
///   stateOnChange(state1, closure((_v) -> textSetString(__h, fresh_concat))),
///   stateOnChange(state2, closure((_v) -> textSetString(__h, fresh_concat))),
///   ...,
///   LocalGet(__h),
/// ])
/// ```
///
/// The concat is re-lowered for each closure so each subscriber reads every
/// state freshly — correct for `Text(\`${a.value} and ${b.value}\`)` where a
/// change to `a` still needs the current value of `b`.
pub(crate) fn try_desugar_reactive_text(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
) -> Result<Option<Expr>> {
    // Callee must be the bare identifier `Text`.
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return Ok(None);
    };
    let ast::Expr::Ident(ident) = callee_expr.as_ref() else {
        return Ok(None);
    };
    if ident.sym.as_ref() != "Text" {
        return Ok(None);
    }
    // `Text` must resolve to `perry/ui`'s Text import. Rejects a user-defined
    // `function Text(...)` or an import from another module.
    match ctx.lookup_native_module("Text") {
        Some(("perry/ui", Some(m))) if m == "Text" => {}
        _ => return Ok(None),
    }
    // Only the 1-arg positional form. Spread or additional config args fall
    // through — avoids clobbering setter-chained call forms that we haven't
    // proven we can reproduce bit-for-bit.
    if call.args.iter().any(|a| a.spread.is_some()) {
        return Ok(None);
    }
    if call.args.len() != 1 {
        return Ok(None);
    }
    let ast::Expr::Tpl(tpl) = call.args[0].expr.as_ref() else {
        return Ok(None);
    };

    // Collect unique `<ident>.value` interpolations where `<ident>` is a
    // State binding. De-dup by name so two references to the same state
    // only register one subscriber.
    let mut state_names: Vec<String> = Vec::new();
    for expr in tpl.exprs.iter() {
        let ast::Expr::Member(member) = expr.as_ref() else {
            continue;
        };
        let ast::MemberProp::Ident(prop) = &member.prop else {
            continue;
        };
        if prop.sym.as_ref() != "value" {
            continue;
        }
        let ast::Expr::Ident(obj_ident) = member.obj.as_ref() else {
            continue;
        };
        let name = obj_ident.sym.to_string();
        let is_state = matches!(
            ctx.lookup_native_instance(&name),
            Some(("perry/ui", "State"))
        );
        if is_state && !state_names.contains(&name) {
            state_names.push(name);
        }
    }
    if state_names.is_empty() {
        return Ok(None);
    }

    // Emit as an IIFE closure so the widget handle can be a *real* function
    // local (backed by a WASM local or LLVM alloca) rather than a bare LocalId
    // floating inside an Expr::Sequence. The WASM backend only registers
    // locals via `Stmt::Let`; a LocalSet/LocalGet pair with no backing Let
    // falls through to TAG_UNDEFINED at read time, which silently drops the
    // widget from its parent container.
    //
    //   (() => {
    //     const __h = Text(concat);
    //     stateOnChange(state1, (__v) => textSetString(__h, concat));
    //     ...
    //     return __h;
    //   })()
    let outer_func_id = ctx.fresh_func();
    let outer_scope = ctx.enter_scope();
    let widget_id = ctx.define_local("__perry_reactive_text_h".to_string(), Type::Any);

    let initial_concat = lower_tpl_to_concat(ctx, tpl)?;
    let text_call = Expr::NativeMethodCall {
        module: "perry/ui".to_string(),
        method: "Text".to_string(),
        object: None,
        args: vec![initial_concat],
        class_name: None,
    };

    let mut outer_body: Vec<Stmt> = Vec::new();
    outer_body.push(Stmt::Let {
        id: widget_id,
        name: "__perry_reactive_text_h".to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(text_call),
    });

    for state_name in &state_names {
        let state_local = ctx
            .lookup_local(state_name)
            .ok_or_else(|| anyhow!("reactive Text: state '{}' not in scope", state_name))?;

        // Inner rebuild closure: (__v) => textSetString(__h, <fresh concat>).
        // A fresh concat is required because the callback reads the *current*
        // state values at fire-time — re-using `initial_concat` would bind to
        // the HIR tree already consumed by the Let above.
        let inner_func_id = ctx.fresh_func();
        let inner_scope = ctx.enter_scope();
        let v_param_id = ctx.define_local("__v".to_string(), Type::Any);
        let v_param = Param {
            id: v_param_id,
            name: "__v".to_string(),
            ty: Type::Any,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        };
        let fresh_concat = lower_tpl_to_concat(ctx, tpl)?;
        let set_text_call = Expr::NativeMethodCall {
            module: "perry/ui".to_string(),
            method: "textSetString".to_string(),
            object: None,
            args: vec![Expr::LocalGet(widget_id), fresh_concat],
            class_name: None,
        };
        let inner_body = vec![Stmt::Expr(set_text_call)];
        ctx.exit_scope(inner_scope);

        let mut inner_refs = Vec::new();
        let mut inner_visited = std::collections::HashSet::new();
        for stmt in &inner_body {
            collect_local_refs_stmt(stmt, &mut inner_refs, &mut inner_visited);
        }
        let mut inner_captures: Vec<LocalId> = inner_refs
            .into_iter()
            .filter(|id| *id != v_param_id)
            .collect();
        inner_captures.sort();
        inner_captures.dedup();
        inner_captures = ctx.filter_module_level_captures(inner_captures);

        let inner_closure = Expr::Closure {
            func_id: inner_func_id,
            params: vec![v_param],
            return_type: Type::Any,
            body: inner_body,
            captures: inner_captures,
            mutable_captures: Vec::new(),
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: false,
            is_async: false,
            is_generator: false,
            is_strict: ctx.current_strict,
        };

        outer_body.push(Stmt::Expr(Expr::NativeMethodCall {
            module: "perry/ui".to_string(),
            method: "stateOnChange".to_string(),
            object: None,
            args: vec![Expr::LocalGet(state_local), inner_closure],
            class_name: None,
        }));
    }

    outer_body.push(Stmt::Return(Some(Expr::LocalGet(widget_id))));
    ctx.exit_scope(outer_scope);

    let mut outer_refs = Vec::new();
    let mut outer_visited = std::collections::HashSet::new();
    for stmt in &outer_body {
        collect_local_refs_stmt(stmt, &mut outer_refs, &mut outer_visited);
    }
    let mut outer_captures: Vec<LocalId> = outer_refs
        .into_iter()
        .filter(|id| *id != widget_id)
        .collect();
    outer_captures.sort();
    outer_captures.dedup();
    outer_captures = ctx.filter_module_level_captures(outer_captures);

    let outer_closure = Expr::Closure {
        func_id: outer_func_id,
        params: vec![],
        return_type: Type::Any,
        body: outer_body,
        captures: outer_captures,
        mutable_captures: Vec::new(),
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: false,
        is_async: false,
        is_generator: false,
        is_strict: ctx.current_strict,
    };

    Ok(Some(Expr::Call {
        callee: Box::new(outer_closure),
        args: vec![],
        type_args: vec![],
    }))
}
