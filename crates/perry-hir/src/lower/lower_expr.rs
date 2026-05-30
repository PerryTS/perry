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

pub(crate) fn lower_expr_assignment(
    ctx: &mut LoweringContext,
    expr: &ast::Expr,
    value: Box<Expr>,
) -> Result<Expr> {
    match expr {
        ast::Expr::Ident(ident) => {
            let name = ident.sym.to_string();
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
                eprintln!(
                    "  Warning: Assignment to undeclared variable '{}', creating implicit local",
                    name
                );
                let id = ctx.define_local(name, Type::Any);
                Ok(Expr::LocalSet(id, value))
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
            let object = Box::new(lower_expr(ctx, &member.obj)?);
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
                    Ok(Expr::PropertySet {
                        object,
                        property,
                        value,
                    })
                }
                ast::MemberProp::Computed(computed) => {
                    let index = Box::new(lower_expr(ctx, &computed.expr)?);
                    Ok(Expr::IndexSet {
                        object,
                        index,
                        value,
                    })
                }
                ast::MemberProp::PrivateName(private) => {
                    let property = format!("#{}", private.name);
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
            if let Some(id) = ctx.lookup_local(&name) {
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
                            // workerData is a property-like import that calls a getter function
                            return Ok(Expr::NativeMethodCall {
                                module: "worker_threads".to_string(),
                                class_name: None,
                                object: None,
                                method: "workerData".to_string(),
                                args: Vec::new(),
                            });
                        }
                        if method == "parentPort" {
                            // parentPort is a singleton handle - call getter function
                            return Ok(Expr::NativeMethodCall {
                                module: "worker_threads".to_string(),
                                class_name: None,
                                object: None,
                                method: "parentPort".to_string(),
                                args: Vec::new(),
                            });
                        }
                    }
                }
                if let Some(method) = method_name {
                    return Ok(Expr::PropertyGet {
                        object: Box::new(Expr::NativeModuleRef(module_name.to_string())),
                        property: method.to_string(),
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
                let path = &ctx.source_file_path;
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
            } else {
                // GlobalGet(0) is a sentinel: codegen routes by name from the
                // parent PropertyGet/Call/Member context. Bare uses lower to
                // 0.0 (perry-codegen/src/expr.rs Expr::GlobalGet arm).
                if name != "console"
                    && name != "process"
                    && name != "globalThis"
                    && name != "Buffer"
                    && name != "Date"
                    && name != "JSON"
                    && name != "Math"
                    && name != "Object"
                    && name != "Array"
                    && name != "String"
                    && name != "Number"
                    && name != "Boolean"
                    && name != "Function"
                    && name != "Error"
                    && name != "TypeError"
                    && name != "RangeError"
                    && name != "SyntaxError"
                    && name != "ReferenceError"
                    && name != "EvalError"
                    && name != "URIError"
                    && name != "AggregateError"
                    && name != "Promise"
                    && name != "Map"
                    && name != "Set"
                    && name != "RegExp"
                    && name != "Symbol"
                    && name != "WeakMap"
                    && name != "WeakSet"
                    && name != "WeakRef"
                    && name != "FinalizationRegistry"
                    && name != "Proxy"
                    && name != "Reflect"
                    && name != "Uint8Array"
                    && name != "Int8Array"
                    && name != "Int16Array"
                    && name != "Uint16Array"
                    && name != "Int32Array"
                    && name != "Uint32Array"
                    && name != "Float32Array"
                    && name != "Float64Array"
                    && name != "TextEncoder"
                    && name != "TextDecoder"
                    && name != "URL"
                    && name != "URLSearchParams"
                    && name != "AbortController"
                    && name != "FormData"
                    && name != "File"
                    && name != "Headers"
                    && name != "fetch"
                    && name != "crypto"
                    && name != "performance"
                    && name != "queueMicrotask"
                    && name != "structuredClone"
                    && name != "atob"
                    && name != "btoa"
                    && name != "BigInt"
                    && name != "WebAssembly"
                    && !is_builtin_global_value_name(&name)
                {
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
                let ty_expr = if let ast::Expr::Ident(ident) = bin.right.as_ref() {
                    let name = ident.sym.as_ref();
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
                    if ctx.lookup_local(name).is_some() || ctx.lookup_func(name).is_some() {
                        match lower_expr(ctx, &bin.right) {
                            Ok(e) => Some(Box::new(e)),
                            Err(_) => None,
                        }
                    } else {
                        None
                    }
                } else {
                    None
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
                // #677: bare `typeof Function` — Function is a JS built-in
                // constructor, so typeof is "function". Without this fold,
                // the bare ident lowers to `GlobalGet(0)` and typeof reads
                // "object" via the global-this short-circuit.
                if let ast::Expr::Ident(id) = unary.arg.as_ref() {
                    if id.sym.as_ref() == "Function" && ctx.lookup_local("Function").is_none() {
                        return Ok(Expr::String("function".to_string()));
                    }
                    // #1454: global timer builtins (+ fetch) are functions, but
                    // a bare read lowers to an ExternFuncRef whose typeof reads
                    // "boolean". Fold to "function" (gc is excluded — it's
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
                }
                // #1395: `typeof process.memoryUsage.rss` is a nested member
                // (`(process.memoryUsage).rss`) so it bypasses the
                // ident-receiver fold below. Node exposes `rss` as a fast-path
                // function hung off `process.memoryUsage`; fold to "function".
                if let ast::Expr::Member(outer) = unary.arg.as_ref() {
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
                if let ast::Expr::Member(member) = unary.arg.as_ref() {
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
                            if obj_name == "process"
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
                                && ctx.lookup_local("process").is_none()
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
            // Check if any elements are spread elements
            let has_spread = array
                .elems
                .iter()
                .filter_map(|elem| elem.as_ref())
                .any(|elem| elem.spread.is_some());

            if has_spread {
                // Use ArraySpread for arrays with spread elements.
                // If a spread source is a generator call, wrap it in IteratorToArray
                // so the codegen gets a real array to iterate.
                let elements = array
                    .elems
                    .iter()
                    .filter_map(|elem| elem.as_ref())
                    .map(|elem| {
                        let expr = lower_expr(ctx, &elem.expr)?;
                        if elem.spread.is_some() {
                            // Wrap generator calls in IteratorToArray
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
                // No spread elements, use regular Array
                let elements = array
                    .elems
                    .iter()
                    .map(|elem| match elem.as_ref() {
                        Some(elem) => lower_expr(ctx, &elem.expr),
                        None => Ok(Expr::Undefined),
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(Expr::Array(elements))
            }
        }
        ast::Expr::Object(obj) => expr_object::lower_object(ctx, obj),
        ast::Expr::This(_) => {
            // Always use Expr::This - the codegen will handle it with ThisContext
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
                            // Mirror the eager-fold from
                            // `expr_member::lower_member` for `m.index` /
                            // `m.groups` when `m` is a tracked
                            // regex.exec()/string.match() result. The
                            // standard `lower_member` fires when the
                            // AST is `m.groups`; for the optional-chain
                            // form `m?.groups` SWC routes here, the
                            // `member.obj` has already been lowered to
                            // `LocalGet(...)`, so `lower_member`'s
                            // `ast::Expr::Ident` check fails. Intercept
                            // here so the optional-chain shape also
                            // resolves to the thread-local groups
                            // object instead of a generic property
                            // read that returns undefined.
                            if (prop_name == "groups" || prop_name == "index")
                                && match member.obj.as_ref() {
                                    ast::Expr::Ident(ident) => {
                                        ctx.regex_exec_locals.contains(&ident.sym.to_string())
                                    }
                                    ast::Expr::TsNonNull(nn) => match nn.expr.as_ref() {
                                        ast::Expr::Ident(ident) => {
                                            ctx.regex_exec_locals.contains(&ident.sym.to_string())
                                        }
                                        _ => false,
                                    },
                                    _ => false,
                                }
                            {
                                if prop_name == "groups" {
                                    Expr::RegExpExecGroups
                                } else {
                                    Expr::RegExpExecIndex
                                }
                            } else {
                                Expr::PropertyGet {
                                    object: Box::new(obj_expr.clone()),
                                    property: prop_name,
                                }
                            }
                        }
                        ast::MemberProp::Computed(comp) => {
                            let index = lower_expr(ctx, &comp.expr)?;
                            Expr::IndexGet {
                                object: Box::new(obj_expr.clone()),
                                index: Box::new(index),
                            }
                        }
                        _ => return Err(anyhow!("Unsupported optional chain property type")),
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
                                    _ => return Err(anyhow!("Unsupported optional chain member")),
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
                                // Chained `foo?.bar?.(args)`: keep checking the
                                // receiver (foo) so the inner `?.` short-circuit
                                // still works. A separate null-check on the
                                // function value (foo.bar) is a known gap — see
                                // the comment above the final Conditional below.
                                ast::OptChainBase::Member(m) => lower_member_flat(m)?,
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
                            callee: Box::new(fixed_callee),
                            args,
                            type_args: Vec::new(),
                        };
                        return Ok(Expr::Conditional {
                            condition: inner_cond,
                            then_expr: inner_then,
                            else_expr: Box::new(outer_call),
                        });
                    }

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
                        else_expr: Box::new(call_expr),
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
            // `TaggedTemplateStrings` codegen emits both arrays + a
            // `js_tagged_template_register_raw` call so `strings.raw` reads
            // can resolve via the matching `Expr::TemplateRaw` fold below.
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
            let synthetic_name =
                ident_name.unwrap_or_else(|| format!("__anon_class_{}", ctx.fresh_class()));
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
            ctx.pending_classes.push(class);
            // #1772: a class EXPRESSION that carries per-evaluation static
            // fields and is NOT a mixin (`class extends <expr>`) lowers to a
            // fresh heap class object per evaluation (`ClassExprFresh`), so
            // `make(a) !== make(b)` and each holds its own statics as own
            // properties. Mixins and static-less class expressions keep the
            // historical (shared-template) path.
            if parent_expr.is_none()
                && (!named_statics.is_empty() || !static_symbol_registrations.is_empty())
            {
                // #1787: snapshot the class's captured outer-scope values so a
                // later `new <classObjectValue>()` can run the instance-field
                // initializers / constructor body with the right environment.
                // `synthesize_class_captures` (run during `lower_class_from_ast`
                // above) appended one `__perry_cap_<id>` constructor param per
                // captured outer id, in `captures_vec` order — read them back in
                // that same order as `LocalGet(outer_id)`, evaluated here where
                // the captures are still live.
                let captured_args: Vec<Expr> = ctx
                    .lookup_class_captures(&synthetic_name)
                    .map(|ids| ids.iter().map(|id| Expr::LocalGet(*id)).collect())
                    .unwrap_or_default();
                return Ok(Expr::ClassExprFresh {
                    template: synthetic_name,
                    named_statics,
                    symbol_statics: static_symbol_registrations,
                    captured_args,
                });
            }
            let mut seq: Vec<Expr> = Vec::new();
            if let Some(p) = parent_expr {
                seq.push(Expr::RegisterClassParentDynamic {
                    class_name: synthetic_name.clone(),
                    parent_expr: p,
                });
            }
            for (k, v) in static_symbol_registrations {
                seq.push(Expr::RegisterClassStaticSymbol {
                    class_name: synthetic_name.clone(),
                    key_expr: Box::new(k),
                    value_expr: Box::new(v),
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
            enclosing_class: None,
            is_async: false,
            is_generator: false,
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
        enclosing_class: None,
        is_async: false,
        is_generator: false,
    };

    Ok(Some(Expr::Call {
        callee: Box::new(outer_closure),
        args: vec![],
        type_args: vec![],
    }))
}
