//! AST to HIR lowering — extracted from `lower/mod.rs` (issue #1101).
//!
//! Pure mechanical split: no logic changes. Helpers keep their original
//! visibility and are re-exported from `lower/mod.rs` so the existing
//! `expr_*` submodules and the rest of the crate keep compiling unchanged.

use crate::types::Type;
use swc_ecma_ast as ast;

use super::*;
use crate::ir::*;

pub(crate) fn string_directive_stmt_lit(stmt: &ast::Stmt) -> Option<&ast::Str> {
    let ast::Stmt::Expr(expr_stmt) = stmt else {
        return None;
    };
    let ast::Expr::Lit(ast::Lit::Str(str_lit)) = expr_stmt.expr.as_ref() else {
        return None;
    };
    Some(str_lit)
}

pub(crate) fn is_raw_use_strict_directive(str_lit: &ast::Str) -> bool {
    // Directive recognition is based on the raw token text. The cooked string
    // value would incorrectly treat escapes like "use\x20strict" as strict.
    matches!(
        str_lit.raw.as_ref().map(|raw| raw.as_ref()),
        Some("\"use strict\"") | Some("'use strict'")
    )
}

fn is_use_strict_directive_stmt(stmt: &ast::Stmt) -> Option<bool> {
    string_directive_stmt_lit(stmt).map(is_raw_use_strict_directive)
}

pub(crate) fn stmt_list_starts_with_use_strict_directive(stmts: &[ast::Stmt]) -> bool {
    for stmt in stmts {
        match is_use_strict_directive_stmt(stmt) {
            Some(true) => return true,
            Some(false) => continue,
            None => return false,
        }
    }
    false
}

pub(crate) fn module_starts_with_use_strict_directive(module: &ast::Module) -> bool {
    for item in &module.body {
        match item {
            ast::ModuleItem::Stmt(stmt) => match is_use_strict_directive_stmt(stmt) {
                Some(true) => return true,
                Some(false) => continue,
                None => return false,
            },
            ast::ModuleItem::ModuleDecl(_) => return false,
        }
    }
    false
}

pub(crate) fn module_has_module_declaration(module: &ast::Module) -> bool {
    module
        .body
        .iter()
        .any(|item| matches!(item, ast::ModuleItem::ModuleDecl(_)))
}

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
            "EventEmitterAsyncResource" => Some(("events", "EventEmitterAsyncResource")),
            "Pool" => Some(("mysql2/promise", "Pool")),
            "PoolConnection" => Some(("mysql2/promise", "PoolConnection")),
            "WebSocket" => Some(("ws", "WebSocket")),
            "WebSocketServer" => Some(("ws", "WebSocketServer")),
            // Fastify: the same (module, class) pairs the *parameter* paths
            // already register (`lower_decl/fn_decl.rs`, `lower/expr_function.rs`).
            // Without these, the extremely common server-factory shape
            //   // server.ts
            //   export function buildServer(): FastifyInstance { return Fastify(); }
            //   // main.ts
            //   const app = buildServer();
            //   await app.listen({ port });   // <- receiver not known to be native
            // loses the native handle across the module boundary: `listen` falls
            // back to `dynamic_boundary:runtime_api` and codegen's unknown-native-
            // method arm SILENTLY returns 0.0, so the process starts, awaits a
            // resolved promise, binds nothing, and exits 0 with no diagnostic.
            "FastifyInstance" => Some(("fastify", "App")),
            "FastifyRequest" => Some(("fastify", "Request")),
            "FastifyReply" => Some(("fastify", "Reply")),
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

/// Function declarations with the same name in the same scope follow JS's
/// "last declaration wins" semantics. Keep the latest HIR body and avoid
/// emitting duplicate LLVM symbols for the same scoped function name.
pub(crate) fn push_function_decl_dedup(module: &mut Module, func: Function) {
    if let Some(existing) = module
        .functions
        .iter_mut()
        .find(|existing| existing.name == func.name)
    {
        *existing = func;
    } else {
        module.functions.push(func);
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
        ast::PropName::Num(n) => number_to_js_key(n.value),
        _ => String::new(),
    }
}

/// ECMA-262 `Number::toString(m)` (base 10) — the canonical property-key
/// spelling of a numeric literal member/key. Rust's `f64::to_string` never uses
/// exponential notation, so `0.0000001` would stringify to `"0.0000001"` while
/// JS (and Perry's runtime key coercion, `js_jsvalue_to_string`) produces
/// `"1e-7"`. A numeric-keyed member registered under the Rust spelling would
/// then be unreachable via `obj[0.0000001]`. This matches the runtime so the
/// two agree (Test262 .../accessor-name-*/literal-numeric-non-canonical etc.).
pub(crate) fn number_to_js_key(m: f64) -> String {
    if m.is_nan() {
        return "NaN".to_string();
    }
    if m == 0.0 {
        return "0".to_string(); // both +0 and -0 → "0"
    }
    if m < 0.0 {
        return format!("-{}", number_to_js_key(-m));
    }
    if m.is_infinite() {
        return "Infinity".to_string();
    }
    // Rust's `{:e}` yields the shortest round-tripping mantissa with a base-10
    // exponent: `d[.ddd]e<exp>` (exp has no leading '+'). Reconstruct ECMA's
    // (digits k, point position n) from it.
    let sci = format!("{:e}", m);
    let (mant, exp_str) = match sci.split_once('e') {
        Some(parts) => parts,
        None => return sci,
    };
    let big_e: i32 = exp_str.parse().unwrap_or(0);
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = big_e + 1;
    if k <= n && n <= 21 {
        let mut s = digits;
        for _ in 0..(n - k) {
            s.push('0');
        }
        s
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        let mut s = String::from("0.");
        for _ in 0..(-n) {
            s.push('0');
        }
        s.push_str(&digits);
        s
    } else {
        let exp = n - 1;
        let sign = if exp >= 0 { "+" } else { "-" };
        if k == 1 {
            format!("{}e{}{}", digits, sign, exp.abs())
        } else {
            format!("{}.{}e{}{}", &digits[..1], &digits[1..], sign, exp.abs())
        }
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
