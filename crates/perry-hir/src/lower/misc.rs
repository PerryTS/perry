//! AST to HIR lowering — extracted from `lower/mod.rs` (issue #1101).
//!
//! Pure mechanical split: no logic changes. Helpers keep their original
//! visibility and are re-exported from `lower/mod.rs` so the existing
//! `expr_*` submodules and the rest of the crate keep compiling unchanged.

#![allow(unused_imports)]

use anyhow::{anyhow, Result};
use perry_types::{FuncId, FunctionType, GlobalId, LocalId, Type, TypeParam};
use std::collections::{HashMap, HashSet};
use swc_ecma_ast as ast;

use super::*;
use crate::ir::*;

/// Map a function's declared return type to a native-instance class when it
/// matches a known stdlib pattern. Lets a wrapper function like
/// `function openSocket(host, port): Socket { ... }` advertise that calls
/// to it produce a Socket instance — call sites then register the local
/// via the user-factory consumer in the var-decl handler, so subsequent
/// `sock.on(...)` / `sock.write(...)` dispatches statically through the
/// NATIVE_MODULE_TABLE just like `const sock = net.createConnection(...)`.
///
/// Recognizes both `T` and `Promise<T>` return types so async wrappers
/// work without ceremony.
pub(crate) fn native_instance_from_return_type(ty: &Type) -> Option<(&'static str, &'static str)> {
    let inner = match ty {
        Type::Generic { base, type_args } if base == "Promise" => type_args.first().unwrap_or(ty),
        Type::Promise(inner) => inner.as_ref(),
        other => other,
    };
    if let Type::Named(name) = inner {
        return match name.as_str() {
            "Socket" => Some(("net", "Socket")),
            "Redis" => Some(("ioredis", "Redis")),
            "EventEmitter" => Some(("events", "EventEmitter")),
            "Pool" => Some(("mysql2/promise", "Pool")),
            "PoolConnection" => Some(("mysql2/promise", "PoolConnection")),
            "WebSocket" => Some(("ws", "WebSocket")),
            "WebSocketServer" => Some(("ws", "WebSocketServer")),
            _ => None,
        };
    }
    None
}

/// Append `class` to `module.classes` only if no class with the same name has
/// already been pushed. Same dedup policy that the function-scoped path at
/// `lower_decl.rs::3059` already enforces — extended here to every push site
/// so namespace-scoped, mixin, and class-expression paths can't smuggle a
/// duplicate-named class into codegen and trip the LLVM IR's "redefinition
/// of global '@perry_class_keys_<modprefix>__<class>'" rule. The lookup
/// pipeline (`Expr::New { class_name }`, `class_lookup` / `class_ids`) is
/// purely name-based today, so the second class wouldn't be reachable through
/// any binding anyway — emitting it just produces unreachable globals that
/// then collide with the first class's globals at link time. See #336.
pub(crate) fn push_class_dedup(module: &mut Module, class: Class) {
    if !module.classes.iter().any(|c| c.name == class.name) {
        module.classes.push(class);
    }
}

/// Fill in `Class::extends_name` for classes whose parent is the result of
/// calling a statically-resolvable factory function — but ONLY when the
/// parent's field initializers are closure-free (no `LocalGet` reads). The
/// post-pass runs after every function and class is in `module`, so
/// forward-references work (e.g. `class Sub extends makeBare() {}` ahead of
/// `function makeBare() …` hoisting).
///
/// The closure-free guard exists because the field-init pass at codegen
/// (`apply_field_initializers_recursive`) inlines each chained class's
/// init expressions directly into the subclass's constructor. That's
/// correct for pure-literal initializers like `kind = "bare"` but wrong
/// for `_tag = tag` where `tag` is the factory's parameter — the inlined
/// `LocalGet(tag)` would re-resolve in the subclass's scope (where `tag`
/// doesn't exist) and produce garbage. Conservatively skip those: the
/// subclass's static parent stays None and field-init inheritance only
/// works for the literal-initialized parents that #806's bare-factory
/// section needs.
pub(crate) fn infer_dynamic_extends_names(module: &mut Module) {
    use std::collections::HashMap;
    // Build a map of `function_id → returned ClassRef name` for every
    // function whose body returns a static ClassRef. Only the LAST `Return`
    // is examined — bodies with multiple Returns to different classes
    // don't resolve uniquely, and the canonical factory shape has exactly
    // one Return as its last statement.
    let mut factory_returns: HashMap<u32, String> = HashMap::new();
    for func in &module.functions {
        if let Some(name) = trailing_return_classref(&func.body) {
            factory_returns.insert(func.id, name);
        }
    }
    // Index classes by name so we can re-resolve transitively (a chain like
    // `Sub extends A() {}` where `A` returns `__anon_N` and `__anon_N` is
    // a class we own — we only set `extends_name` for `Sub` here; chain
    // walks at codegen step through `__anon_N.extends_name` normally).
    let class_field_inits_pure: HashMap<String, bool> = module
        .classes
        .iter()
        .map(|c| (c.name.clone(), fields_are_pure(c)))
        .collect();
    for class in &mut module.classes {
        if class.extends_name.is_some() {
            continue;
        }
        let Some(expr) = class.extends_expr.as_deref() else {
            continue;
        };
        let Expr::Call { callee, .. } = expr else {
            continue;
        };
        let Expr::FuncRef(func_id) = callee.as_ref() else {
            continue;
        };
        let Some(parent_name) = factory_returns.get(func_id) else {
            continue;
        };
        // Only inherit field-init machinery when the parent's fields are
        // pure (no `LocalGet`). Methods on the parent are unaffected —
        // those dispatch through the runtime CLASS_REGISTRY which is
        // populated by the #826 RegisterClassParentDynamic side effect.
        if class_field_inits_pure
            .get(parent_name)
            .copied()
            .unwrap_or(false)
        {
            class.extends_name = Some(parent_name.clone());
        }
    }
}

/// True when none of the class's field initializers contain a `LocalGet`
/// (the canonical sign that an initializer closes over its surrounding
/// scope — function parameters, outer-block lets, etc.).
fn fields_are_pure(class: &Class) -> bool {
    for field in &class.fields {
        if let Some(init) = &field.init {
            if expr_reads_local(init) {
                return false;
            }
        }
        if let Some(key) = &field.key_expr {
            if expr_reads_local(key) {
                return false;
            }
        }
    }
    true
}

fn expr_reads_local(expr: &Expr) -> bool {
    if matches!(expr, Expr::LocalGet(_)) {
        return true;
    }
    let mut found = false;
    crate::walker::walk_expr_children(expr, &mut |child| {
        if !found && expr_reads_local(child) {
            found = true;
        }
    });
    found
}

/// Return `Some(name)` if `body`'s last `Return` statement yields a static
/// `Expr::ClassRef` (directly or as the last element of an `Expr::Sequence`).
fn trailing_return_classref(body: &[Stmt]) -> Option<String> {
    for stmt in body.iter().rev() {
        if let Stmt::Return(Some(expr)) = stmt {
            return classref_name(expr);
        }
    }
    None
}

fn classref_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::ClassRef(name) => Some(name.clone()),
        Expr::Sequence(parts) => parts.last().and_then(classref_name),
        _ => None,
    }
}

/// Extract a property name from a PropName
pub(crate) fn prop_name_to_string(name: &ast::PropName) -> String {
    match name {
        ast::PropName::Ident(ident) => ident.sym.to_string(),
        ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
        ast::PropName::Num(n) => format!("{}", n.value),
        _ => String::new(),
    }
}

/// Detect whether an AST expression statically produces a string value.
///
/// Used to specialize `for...of` and array-spread lowering when the iterable is
/// a string — in that case we need char-by-char iteration via `str[i]` rather
/// than array-element access.
/// Check if a lowered HIR expression is a call to a generator function.
pub(super) fn is_generator_call_expr(ctx: &LoweringContext, expr: &Expr) -> bool {
    if let Expr::Call { callee, .. } = expr {
        if let Expr::FuncRef(func_id) = callee.as_ref() {
            // Look up the function name by its ID
            for (name, id) in &ctx.functions {
                if *id == *func_id && ctx.generator_func_names.contains(name) {
                    return true;
                }
            }
        }
        // #321: a generator function EXPRESSION bound to a name (`const range =
        // function*(){}`) lowers `range()` to `Call { callee: LocalGet(id) }`,
        // not a `FuncRef`. Resolve the local's name and check the same set the
        // for-of path registers into (see destructuring/var_decl.rs).
        if let Expr::LocalGet(local_id) = callee.as_ref() {
            if let Some((name, _, _)) = ctx.locals.iter().find(|(_, id, _)| id == local_id) {
                if ctx.generator_func_names.contains(name) {
                    return true;
                }
            }
        }
    }
    false
}

pub(crate) fn is_ast_string_expr(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    match expr {
        // String literals: "hello"
        ast::Expr::Lit(ast::Lit::Str(_)) => true,
        // Template literals: `hello ${x}`
        ast::Expr::Tpl(_) => true,
        // String identifier: look up the declared type in the current scope
        ast::Expr::Ident(ident) => {
            let name = ident.sym.to_string();
            matches!(ctx.lookup_local_type(&name), Some(Type::String))
        }
        // Parenthesized expression: recurse
        ast::Expr::Paren(p) => is_ast_string_expr(ctx, &p.expr),
        // Type assertions (`x as string`): check inner
        ast::Expr::TsAs(ts_as) => {
            if matches!(&*ts_as.type_ann,
                ast::TsType::TsKeywordType(kw)
                    if matches!(kw.kind, ast::TsKeywordTypeKind::TsStringKeyword))
            {
                return true;
            }
            is_ast_string_expr(ctx, &ts_as.expr)
        }
        ast::Expr::TsNonNull(nn) => is_ast_string_expr(ctx, &nn.expr),
        ast::Expr::TsTypeAssertion(ta) => is_ast_string_expr(ctx, &ta.expr),
        // String-returning method calls on string receivers
        ast::Expr::Call(call) => {
            if let ast::Callee::Expr(callee_expr) = &call.callee {
                if let ast::Expr::Member(member) = callee_expr.as_ref() {
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        let prop = prop_ident.sym.as_ref();
                        if matches!(
                            prop,
                            "charAt"
                                | "slice"
                                | "substring"
                                | "substr"
                                | "trim"
                                | "trimStart"
                                | "trimEnd"
                                | "toLowerCase"
                                | "toUpperCase"
                                | "replace"
                                | "replaceAll"
                                | "padStart"
                                | "padEnd"
                                | "repeat"
                                | "normalize"
                                | "concat"
                                | "toString"
                                | "toLocaleLowerCase"
                                | "toLocaleUpperCase"
                        ) {
                            return is_ast_string_expr(ctx, &member.obj);
                        }
                    }
                }
            }
            false
        }
        // String concatenation: "a" + x or x + "a"
        ast::Expr::Bin(bin) if matches!(bin.op, ast::BinaryOp::Add) => {
            is_ast_string_expr(ctx, &bin.left) || is_ast_string_expr(ctx, &bin.right)
        }
        _ => false,
    }
}

/// Detect whether a var initializer is `regex.exec(str)` (after stripping
/// non-null assertion `!`). Used to mark locals so subsequent `.index`/`.groups`
/// accesses can route to the bare RegExpExecIndex/Groups HIR variants.
pub(crate) fn is_regex_exec_init(ctx: &LoweringContext, init: &ast::Expr) -> bool {
    let expr = match init {
        ast::Expr::TsNonNull(nn) => nn.expr.as_ref(),
        other => other,
    };
    if let ast::Expr::Call(call) = expr {
        if let ast::Callee::Expr(callee) = &call.callee {
            if let ast::Expr::Member(member) = callee.as_ref() {
                if let ast::MemberProp::Ident(method) = &member.prop {
                    let name = method.sym.as_ref();
                    // `regex.exec(str)` — receiver is RegExp.
                    if name == "exec" {
                        return match member.obj.as_ref() {
                            ast::Expr::Lit(ast::Lit::Regex(_)) => true,
                            ast::Expr::Ident(ident) => ctx
                                .lookup_local_type(ident.sym.as_ref())
                                .map(|ty| matches!(ty, Type::Named(n) if n == "RegExp"))
                                .unwrap_or(false),
                            _ => false,
                        };
                    }
                    // `str.match(regex)` — receiver is String (or
                    // untyped/Any). The match result has the same
                    // .index / .groups shape as exec() when the regex
                    // isn't global, so reuse the same thread-local
                    // pickup (LAST_EXEC_GROUPS). The match runtime
                    // stores groups there alongside exec.
                    if name == "match" {
                        let recv_ok = match member.obj.as_ref() {
                            ast::Expr::Lit(ast::Lit::Str(_)) | ast::Expr::Tpl(_) => true,
                            ast::Expr::Ident(ident) => {
                                // Accept String OR Any/unknown — false
                                // positives are limited to user-defined
                                // `.match()` on Any-typed receivers and
                                // their `.groups` reads naturally fall
                                // through to undefined when no string-
                                // match preceded them.
                                let ty = ctx.lookup_local_type(ident.sym.as_ref());
                                matches!(ty, Some(Type::String) | Some(Type::Any) | None)
                            }
                            _ => false,
                        };
                        // Skip global regex matches — match() with /g
                        // returns a flat array of full matches with no
                        // group metadata. Detect via a regex-literal
                        // arg with `g` flag; an Ident arg (regex stored
                        // in a variable) is optimistically treated as
                        // non-global since the common shape is `str.match(/.../)`.
                        if recv_ok {
                            let first_arg_is_global_regex =
                                call.args.first().and_then(|arg| match arg.expr.as_ref() {
                                    ast::Expr::Lit(ast::Lit::Regex(rx)) => {
                                        Some(rx.flags.as_ref().contains('g'))
                                    }
                                    _ => None,
                                });
                            return !matches!(first_arg_is_global_regex, Some(true));
                        }
                    }
                }
            }
        }
    }
    false
}
