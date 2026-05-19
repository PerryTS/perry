//! Pattern and literal lowering utilities.
//!
//! Contains functions for lowering literals, assignment targets, binding names,
//! parameter destructuring, and other pattern-related utilities.

use crate::ir::*;
use crate::lower::{lower_expr, LoweringContext};
use crate::lower_types::*;
use anyhow::{anyhow, Result};
use perry_types::{LocalId, Type};
use swc_common::Spanned;
use swc_ecma_ast as ast;

pub(crate) fn unescape_template(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('r') => result.push('\r'),
                Some('\\') => result.push('\\'),
                Some('$') => result.push('$'),
                Some('`') => result.push('`'),
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }

    result
}

pub(crate) fn lower_lit(lit: &ast::Lit) -> Result<Expr> {
    match lit {
        ast::Lit::Num(n) => {
            let value = n.value;
            // Check if this is an integer that fits in i64
            if value.fract() == 0.0 && value >= i64::MIN as f64 && value <= i64::MAX as f64 {
                Ok(Expr::Integer(value as i64))
            } else {
                Ok(Expr::Number(value))
            }
        }
        ast::Lit::Str(s) => {
            if let Some(valid_utf8) = s.value.as_str() {
                Ok(Expr::String(valid_utf8.to_string()))
            } else {
                // Lone surrogates (U+D800..U+DFFF): SWC stores them as WTF-8 bytes.
                // as_str() returns None because they can't be represented as valid UTF-8.
                Ok(Expr::WtfString(s.value.as_bytes().to_vec()))
            }
        }
        ast::Lit::Bool(b) => Ok(Expr::Bool(b.value)),
        ast::Lit::Null(_) => Ok(Expr::Null),
        ast::Lit::BigInt(bi) => Ok(Expr::BigInt(bi.value.to_string())),
        ast::Lit::Regex(re) => Ok(Expr::RegExp {
            pattern: re.exp.to_string(),
            flags: re.flags.to_string(),
        }),
        _ => Err(anyhow!("Unsupported literal type")),
    }
}

/// Convert an assignment target to an expression for reading its current value
/// Used for compound assignment operators like += to read the current value before modifying
pub(crate) fn lower_assign_target_to_expr(
    ctx: &mut LoweringContext,
    target: &ast::AssignTarget,
) -> Result<Expr> {
    match target {
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(ident)) => {
            let name = ident.id.sym.to_string();
            if let Some(id) = ctx.lookup_local(&name) {
                Ok(Expr::LocalGet(id))
            } else {
                Err(anyhow!(
                    "Undefined variable in compound assignment: {}",
                    name
                ))
            }
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::Member(member)) => {
            // Check if this is a static field access
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                let obj_name = obj_ident.sym.to_string();
                if ctx.lookup_class(&obj_name).is_some() {
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        let field_name = prop_ident.sym.to_string();
                        if ctx.has_static_field(&obj_name, &field_name) {
                            return Ok(Expr::StaticFieldGet {
                                class_name: obj_name,
                                field_name,
                            });
                        }
                    }
                }
            }

            let object = Box::new(lower_expr(ctx, &member.obj)?);
            match &member.prop {
                ast::MemberProp::Ident(ident) => {
                    let property = ident.sym.to_string();
                    Ok(Expr::PropertyGet { object, property })
                }
                ast::MemberProp::Computed(computed) => {
                    let index = Box::new(lower_expr(ctx, &computed.expr)?);
                    Ok(Expr::IndexGet { object, index })
                }
                ast::MemberProp::PrivateName(private) => {
                    let property = format!("#{}", private.name);
                    Ok(Expr::PropertyGet { object, property })
                }
            }
        }
        // Unwrap TypeScript type annotations and parentheses to get the real target
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::Paren(paren)) => {
            lower_expr(ctx, &paren.expr)
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsAs(ts_as)) => {
            lower_expr(ctx, &ts_as.expr)
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsNonNull(ts_nn)) => {
            lower_expr(ctx, &ts_nn.expr)
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsTypeAssertion(ts_ta)) => {
            lower_expr(ctx, &ts_ta.expr)
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsSatisfies(ts_sat)) => {
            lower_expr(ctx, &ts_sat.expr)
        }
        _ => Err(anyhow!("Unsupported target in compound assignment")),
    }
}

pub(crate) fn get_binding_name(pat: &ast::Pat) -> Result<String> {
    match pat {
        ast::Pat::Ident(ident) => Ok(ident.id.sym.to_string()),
        _ => {
            crate::lower_bail!(pat.span(), "Unsupported binding pattern");
        }
    }
}

/// Static counter for generating unique synthetic names for destructuring patterns
static DESTRUCT_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub(crate) fn get_pat_name(pat: &ast::Pat) -> Result<String> {
    match pat {
        ast::Pat::Ident(ident) => Ok(ident.id.sym.to_string()),
        ast::Pat::Assign(assign) => get_pat_name(&assign.left),
        ast::Pat::Rest(rest) => get_pat_name(&rest.arg),
        // For complex destructuring patterns, generate synthetic names
        // The actual destructuring will be handled at the call site or as a separate pass
        ast::Pat::Array(_) => {
            let id = DESTRUCT_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(format!("__arr_destruct_{}", id))
        }
        ast::Pat::Object(_) => {
            let id = DESTRUCT_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(format!("__obj_destruct_{}", id))
        }
        _ => Err(anyhow!("Unsupported pattern")),
    }
}

/// Extract the type annotation from a Pat (for arrow function parameters)
pub(crate) fn get_pat_type(pat: &ast::Pat, ctx: &LoweringContext) -> Type {
    match pat {
        ast::Pat::Ident(ident) => ident
            .type_ann
            .as_ref()
            .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, Some(ctx)))
            .unwrap_or(Type::Any),
        ast::Pat::Assign(assign) => get_pat_type(&assign.left, ctx),
        ast::Pat::Rest(rest) => rest
            .type_ann
            .as_ref()
            .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, Some(ctx)))
            .unwrap_or(Type::Any),
        ast::Pat::Array(arr) => arr
            .type_ann
            .as_ref()
            .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, Some(ctx)))
            .unwrap_or(Type::Any),
        ast::Pat::Object(obj) => obj
            .type_ann
            .as_ref()
            .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, Some(ctx)))
            .unwrap_or(Type::Any),
        _ => Type::Any,
    }
}

/// Generate Let statements to extract destructured variables from a synthetic parameter.
/// For array patterns like `[a, b]`, generates:
///   let a = param[0];
///   let b = param[1];
/// For object patterns like `{a, b}`, generates:
///   let a = param.a;
///   let b = param.b;
/// Delegates to the recursive `lower_pattern_binding` helper so that nested
/// patterns, defaults, rest, and computed keys all work consistently.
pub(crate) fn generate_param_destructuring_stmts(
    ctx: &mut LoweringContext,
    pat: &ast::Pat,
    param_id: LocalId,
) -> Result<Vec<Stmt>> {
    match pat {
        ast::Pat::Array(_) | ast::Pat::Object(_) => {
            crate::destructuring::lower_pattern_binding(ctx, pat, Expr::LocalGet(param_id), false)
        }
        _ => Ok(Vec::new()),
    }
}

/// Check if a pattern is a destructuring pattern (array or object)
pub(crate) fn is_destructuring_pattern(pat: &ast::Pat) -> bool {
    matches!(pat, ast::Pat::Array(_) | ast::Pat::Object(_))
}

/// Detect fastify route-handler calls (`app.get|post|put|delete|patch|head|
/// options|all|addHook(path, handler)` and `app.setErrorHandler(handler)`)
/// and return the names of the first two arrow-function params — which
/// should be registered as fastify `Request` and `Reply` native instances
/// so that `request.header(...)`, `request.headers[...]`, `reply.send(...)`
/// etc. inside the handler body dispatch through the fastify FFI instead
/// of falling through to generic object access.
///
/// Returns `Some((request_name, reply_name))` when the pattern matches,
/// where `reply_name` is empty if the handler takes only one param.
/// Returns `None` for any other call shape.
pub(crate) fn pre_scan_fastify_handler_params(
    ctx: &crate::lower::LoweringContext,
    call: &ast::CallExpr,
) -> Option<(String, String)> {
    use ast::Callee;
    let callee_expr = match &call.callee {
        Callee::Expr(e) => e,
        _ => return None,
    };
    // Callee must be `<obj>.<method>` where <obj> is a registered fastify
    // App (or a chain that resolves to one; for simplicity we only handle
    // the direct Ident case here — app.get(...), not getApp().get(...)).
    let member = match callee_expr.as_ref() {
        ast::Expr::Member(m) => m,
        _ => return None,
    };
    let obj_ident = match member.obj.as_ref() {
        ast::Expr::Ident(i) => i,
        _ => return None,
    };
    let obj_name = obj_ident.sym.to_string();
    let native = ctx.lookup_native_instance(&obj_name)?;
    if native.0 != "fastify" {
        return None;
    }
    let method_name = match &member.prop {
        ast::MemberProp::Ident(i) => i.sym.to_string(),
        _ => return None,
    };
    // The handler is the last arg for route methods (skip the path arg).
    // - `app.get(path, handler)`    → handler_idx = 1
    // - `app.setErrorHandler(hnd)`  → handler_idx = 0
    // - `app.addHook(name, hnd)`    → handler_idx = 1
    let handler_idx = match method_name.as_str() {
        "get" | "post" | "put" | "delete" | "patch" | "head" | "options" | "all" => 1,
        "addHook" => 1,
        "setErrorHandler" => 0,
        _ => return None,
    };
    let handler_arg = call.args.get(handler_idx)?;
    if handler_arg.spread.is_some() {
        return None;
    }
    let arrow = match handler_arg.expr.as_ref() {
        ast::Expr::Arrow(a) => a,
        ast::Expr::Fn(_) => return None, // fn expressions handled separately
        _ => return None,
    };
    // Issue #1070: `setErrorHandler(async (err, req, reply) => …)` —
    // the first arrow param is the THROWN VALUE, not a fastify Request.
    // Registering `err` as `("fastify", "Request")` causes `err.problem`
    // (or any user-field access on a thrown class instance) to lower to
    // a NativeMethodCall whose method name isn't in the fastify Request
    // dispatch table → the lower_native_method_call fall-through emits
    // `double_literal(0.0)`, so the access prints as `0`. Skip the first
    // arrow param for setErrorHandler so only params[1] / params[2]
    // (the real Request / Reply) get the native-instance tags.
    let (req_param_idx, reply_param_idx) = if method_name == "setErrorHandler" {
        (1, 2)
    } else {
        (0, 1)
    };
    let req_name = arrow.params.get(req_param_idx).and_then(pat_ident_name)?;
    let reply_name = arrow
        .params
        .get(reply_param_idx)
        .and_then(pat_ident_name)
        .unwrap_or_default();
    Some((req_name, reply_name))
}

/// Extract a plain-ident name from an arrow function param (skip
/// destructured / rest params — those aren't Request/Reply by shape).
fn pat_ident_name(pat: &ast::Pat) -> Option<String> {
    match pat {
        ast::Pat::Ident(i) => Some(i.id.sym.to_string()),
        _ => None,
    }
}

/// Pre-scan for `http.createServer((req, res) => ...)` and
/// `createServer((req, res) => ...)` (named import from node:http).
/// Issue #577 mirror of `pre_scan_fastify_handler_params`. Returns
/// the (request_local, response_local) names so the caller can
/// register them as `("http", "IncomingMessage")` and
/// `("http", "ServerResponse")` native instances BEFORE the arrow
/// body is lowered — that way `req.method` / `res.end(...)` inside
/// the handler dispatch through NATIVE_MODULE_TABLE.
///
/// Returns `None` for any other call shape.
pub(crate) fn pre_scan_node_http_create_server_params(
    ctx: &crate::lower::LoweringContext,
    call: &ast::CallExpr,
) -> Option<(String, String)> {
    use ast::Callee;
    let callee_expr = match &call.callee {
        Callee::Expr(e) => e,
        _ => return None,
    };

    let (module_name, method_name) = match callee_expr.as_ref() {
        ast::Expr::Member(member) => {
            let obj_ident = match member.obj.as_ref() {
                ast::Expr::Ident(i) => i,
                _ => return None,
            };
            let obj_name = obj_ident.sym.to_string();
            let (module, _) = ctx.lookup_native_module(&obj_name)?;
            let method = match &member.prop {
                ast::MemberProp::Ident(i) => i.sym.to_string(),
                _ => return None,
            };
            (module.to_string(), method)
        }
        ast::Expr::Ident(ident) => {
            let func_name = ident.sym.to_string();
            let (module, method_opt) = ctx.lookup_native_module(&func_name)?;
            let method = method_opt?.to_string();
            (module.to_string(), method)
        }
        _ => return None,
    };

    let _matched = match (module_name.as_str(), method_name.as_str()) {
        ("http", "createServer") => true,
        ("https", "createServer") => true,
        ("http2", "createSecureServer") => true,
        _ => return None,
    };

    let handler_arg = call.args.last()?;
    if handler_arg.spread.is_some() {
        return None;
    }
    let arrow = match handler_arg.expr.as_ref() {
        ast::Expr::Arrow(a) => a,
        _ => return None,
    };
    let req_name = arrow.params.first().and_then(pat_ident_name)?;
    let res_name = arrow
        .params
        .get(1)
        .and_then(pat_ident_name)
        .unwrap_or_default();
    if res_name.is_empty() {
        return None;
    }
    Some((req_name, res_name))
}

/// Pre-scan for `http.get(url, (res) => …)` / `http.request(opts, (res) =>
/// …)` / `https.get` / `https.request`. Issue #1124 followup — the
/// `data` / `end` listeners on the IncomingMessage that arrives at the
/// response callback need to dispatch via NATIVE_MODULE_TABLE entries
/// (class_filter = Some("IncomingMessage")). Pre-fix the `(res)` param
/// was untagged so `res.on('data', cb)` fell through to
/// `js_native_call_method` → small-handle dispatch → no IncomingMessage
/// `on` arm → listener never registered → `'end'` never fired and the
/// (post-#1124-followup) Buffer body never flowed to the user.
///
/// Mirrors `pre_scan_node_http_create_server_params` shape but for the
/// CLIENT factory + single-param `(res)` arrow shape.
///
/// Returns `Some(res_local_name)` when the pattern matches.
pub(crate) fn pre_scan_node_http_client_callback_params(
    ctx: &crate::lower::LoweringContext,
    call: &ast::CallExpr,
) -> Option<String> {
    use ast::Callee;
    let callee_expr = match &call.callee {
        Callee::Expr(e) => e,
        _ => return None,
    };

    let (module_name, method_name) = match callee_expr.as_ref() {
        ast::Expr::Member(member) => {
            let obj_ident = match member.obj.as_ref() {
                ast::Expr::Ident(i) => i,
                _ => return None,
            };
            let obj_name = obj_ident.sym.to_string();
            let (module, _) = ctx.lookup_native_module(&obj_name)?;
            let method = match &member.prop {
                ast::MemberProp::Ident(i) => i.sym.to_string(),
                _ => return None,
            };
            (module.to_string(), method)
        }
        ast::Expr::Ident(ident) => {
            let func_name = ident.sym.to_string();
            let (module, method_opt) = ctx.lookup_native_module(&func_name)?;
            let method = method_opt?.to_string();
            (module.to_string(), method)
        }
        _ => return None,
    };

    // Only http/https request/get factories. http2's `connect()` returns a
    // ClientHttp2Session — different surface, separate pre-scan.
    let _matched = match (module_name.as_str(), method_name.as_str()) {
        ("http", "get" | "request") => true,
        ("https", "get" | "request") => true,
        _ => return None,
    };

    // The response callback is the last arrow/function arg. Walk
    // backwards so options-then-cb shapes (`http.request(opts, cb)`)
    // and url-then-cb shapes (`http.get(url, cb)`) both resolve.
    let handler_arg = call.args.last()?;
    if handler_arg.spread.is_some() {
        return None;
    }
    let arrow = match handler_arg.expr.as_ref() {
        ast::Expr::Arrow(a) => a,
        _ => return None,
    };
    // First (and typically only) arrow param is the IncomingMessage.
    arrow.params.first().and_then(pat_ident_name)
}

/// Pre-scan for `httpServer.on('upgrade', (req, wsId, head) => …)`
/// (issue #577 Phase 4). When the receiver is a registered HttpServer
/// native instance and the event name is `'upgrade'`, register the
/// SECOND arrow param (`wsId`) as a `("ws", "Client")` native instance
/// BEFORE the body is lowered, so calls inside the handler like
/// `wsId.send(...)` / `wsId.on('message', cb)` / `wsId.close()`
/// dispatch through the dedicated Client-class entries in
/// NATIVE_MODULE_TABLE (which call the `js_ws_send_client_i64` /
/// `js_ws_close_client_i64` / `js_ws_on_client_i64` shims that take
/// the receiver as `i64` after `unbox_to_i64` — the wsId arrives
/// from the upgrade dispatch NaN-boxed POINTER_TAG so the unbox
/// extracts the raw integer correctly).
///
/// Returns `Some(wsId_local_name)` when the pattern matches.
pub(crate) fn pre_scan_node_http_upgrade_params(
    ctx: &crate::lower::LoweringContext,
    call: &ast::CallExpr,
) -> Option<String> {
    use ast::Callee;
    let callee_expr = match &call.callee {
        Callee::Expr(e) => e,
        _ => return None,
    };
    let member = match callee_expr.as_ref() {
        ast::Expr::Member(m) => m,
        _ => return None,
    };
    let obj_ident = match member.obj.as_ref() {
        ast::Expr::Ident(i) => i,
        _ => return None,
    };
    let obj_name = obj_ident.sym.to_string();
    let (module, class) = ctx.lookup_native_instance(&obj_name)?;
    if module != "http" || class != "HttpServer" {
        return None;
    }
    let method_name = match &member.prop {
        ast::MemberProp::Ident(i) => i.sym.to_string(),
        _ => return None,
    };
    if method_name != "on" && method_name != "addListener" {
        return None;
    }
    // First arg must be the literal string "upgrade".
    let event_arg = call.args.first()?;
    let event_name = match event_arg.expr.as_ref() {
        ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str().unwrap_or(""),
        _ => return None,
    };
    if event_name != "upgrade" {
        return None;
    }
    // Second arg = handler. Pull the second param (wsId).
    let handler_arg = call.args.get(1)?;
    if handler_arg.spread.is_some() {
        return None;
    }
    let arrow = match handler_arg.expr.as_ref() {
        ast::Expr::Arrow(a) => a,
        _ => return None,
    };
    let ws_id_name = arrow.params.get(1).and_then(pat_ident_name)?;
    Some(ws_id_name)
}

/// Detect if an expression represents a native handle instance (Big, Decimal, etc.)
/// Returns the module name if it does.
///
/// A user `class Big {...}` (or `Decimal`, etc.) in the current module shadows
/// the hardcoded library-name mapping — without that gate `class Big { f0=0; }
/// const b = new Big(); b.f0` returned 0 because the value was routed through
/// big.js's handle-based dispatch.
pub(crate) fn detect_native_instance_expr(
    ctx: &LoweringContext,
    expr: &ast::Expr,
) -> Option<&'static str> {
    match expr {
        // new Big(...) / new Decimal(...) / new BigNumber(...)
        ast::Expr::New(new_expr) => {
            if let ast::Expr::Ident(ident) = new_expr.callee.as_ref() {
                let class_name = ident.sym.as_ref();
                if ctx.classes_index.contains_key(class_name)
                    || ctx.pending_classes.iter().any(|c| c.name == class_name)
                {
                    return None;
                }
                match class_name {
                    "Big" => Some("big.js"),
                    "Decimal" => Some("decimal.js"),
                    "BigNumber" => Some("bignumber.js"),
                    "LRUCache" => Some("lru-cache"),
                    "Command" => Some("commander"),
                    _ => None,
                }
            } else {
                None
            }
        }
        // Chained method calls: new Big(...).plus(...).div(...)
        ast::Expr::Call(call_expr) => {
            if let ast::Callee::Expr(callee_expr) = &call_expr.callee {
                if let ast::Expr::Member(member) = callee_expr.as_ref() {
                    // Recursively check the object
                    detect_native_instance_expr(ctx, &member.obj)
                } else {
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Check if a parameter pattern is a rest parameter (...args)
pub(crate) fn is_rest_param(pat: &ast::Pat) -> bool {
    matches!(pat, ast::Pat::Rest(_))
}

/// Extract default value from a parameter pattern (if any)
/// For optional parameters (x?: Type), we provide Expr::Undefined as the default
pub(crate) fn get_param_default(ctx: &mut LoweringContext, pat: &ast::Pat) -> Result<Option<Expr>> {
    match pat {
        ast::Pat::Ident(ident) => {
            // Check if this is an optional parameter (x?: Type)
            if ident.optional {
                Ok(Some(Expr::Undefined))
            } else {
                Ok(None)
            }
        }
        ast::Pat::Assign(assign) => {
            let default_expr = lower_expr(ctx, &assign.right)?;
            Ok(Some(default_expr))
        }
        _ => Ok(None),
    }
}

/// Built-in Node.js modules that are handled specially by the compiler
const BUILTIN_MODULES: &[&str] = &["fs", "path", "crypto"];

/// Check if an expression is a require() call for a built-in module.
/// Returns the module name if it is, None otherwise.
pub(crate) fn is_require_builtin_module(expr: &ast::Expr) -> Option<String> {
    if let ast::Expr::Call(call) = expr {
        if let ast::Callee::Expr(callee_expr) = &call.callee {
            if let ast::Expr::Ident(ident) = callee_expr.as_ref() {
                if ident.sym.as_ref() == "require" {
                    // Check if the first argument is a string literal
                    if let Some(arg) = call.args.first() {
                        if let ast::Expr::Lit(ast::Lit::Str(s)) = &*arg.expr {
                            let module_name = s.value.as_str().unwrap_or("").to_string();
                            if BUILTIN_MODULES.contains(&module_name.as_str()) {
                                return Some(module_name);
                            }
                        }
                    }
                }
            }
        }
    }
    None
}
