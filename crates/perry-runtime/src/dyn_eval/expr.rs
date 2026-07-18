//! Expression evaluator for #6559.
//!
//! Supported expression subset: identifiers (scope chain → real globals),
//! `this`, literals (string/number/bool/null/regex), template literals,
//! array/object literals (incl. spread), function/arrow expressions
//! (closures over interpreter scopes), unary/update/binary/logical/ternary/
//! sequence operators, `typeof`/`instanceof`/`in`/`delete`, assignments
//! (plain, compound, logical, destructuring), member + computed access,
//! optional chaining, calls (host functions, host methods, interpreted
//! closures — with `this` bound like the runtime binds it), and `new` on
//! host constructors / builtin error types / RegExp.
//!
//! Everything else throws the #6559 diagnostic naming the construct.

use perry_parser::swc_ecma_ast as ast;

use super::bridge::{self, throw_unsupported};
use super::interp::{bind_pattern, make_function_value, Ctx};
use super::{env, root_get, root_push, roots_truncate, InterpBody};

pub(crate) fn eval_expr(ctx: &Ctx, expr: &ast::Expr, env_idx: usize) -> f64 {
    use ast::Expr::*;
    match expr {
        Paren(p) => eval_expr(ctx, &p.expr, env_idx),
        Ident(i) => eval_ident(ctx, &i.sym, env_idx),
        This(_) => root_get(ctx.this_idx),
        Lit(l) => eval_lit(l),
        Tpl(t) => eval_template(ctx, t, env_idx),
        Array(a) => eval_array_lit(ctx, a, env_idx),
        Object(o) => eval_object_lit(ctx, o, env_idx),
        Fn(f) => {
            if f.function.is_generator || f.function.is_async {
                throw_unsupported("generator/async function expression");
            }
            make_function_value(
                ctx,
                f.function.params.iter().map(|p| p.pat.clone()).collect(),
                InterpBody::Block(
                    f.function
                        .body
                        .as_ref()
                        .map(|b| b.stmts.clone())
                        .unwrap_or_default(),
                ),
                false,
                f.ident.as_ref().map(|i| i.sym.to_string()),
                f.function.as_ref() as *const ast::Function as usize,
                env_idx,
            )
        }
        Arrow(a) => {
            if a.is_generator || a.is_async {
                throw_unsupported("generator/async arrow function");
            }
            let body = match a.body.as_ref() {
                ast::BlockStmtOrExpr::BlockStmt(b) => InterpBody::Block(b.stmts.clone()),
                ast::BlockStmtOrExpr::Expr(e) => InterpBody::Expr(e.clone()),
            };
            make_function_value(
                ctx,
                a.params.clone(),
                body,
                true,
                None,
                a as *const ast::ArrowExpr as usize,
                env_idx,
            )
        }
        Unary(u) => eval_unary(ctx, u, env_idx),
        Update(u) => eval_update(ctx, u, env_idx),
        Bin(b) => eval_binary(ctx, b, env_idx),
        Assign(a) => eval_assign(ctx, a, env_idx),
        Cond(c) => {
            let test = eval_expr(ctx, &c.test, env_idx);
            if bridge::truthy(test) {
                eval_expr(ctx, &c.cons, env_idx)
            } else {
                eval_expr(ctx, &c.alt, env_idx)
            }
        }
        Member(m) => eval_member(ctx, m, env_idx),
        Call(c) => eval_call(ctx, c, env_idx),
        New(n) => eval_new(ctx, n, env_idx),
        Seq(s) => {
            let mut last = bridge::undefined();
            for e in &s.exprs {
                last = eval_expr(ctx, e, env_idx);
            }
            last
        }
        OptChain(o) => eval_opt_chain(ctx, o, env_idx),
        Await(_) => throw_unsupported("await (async interpreted code)"),
        Yield(_) => throw_unsupported("yield (generator interpreted code)"),
        Class(_) => throw_unsupported("class expression"),
        TaggedTpl(_) => throw_unsupported("tagged template literal"),
        SuperProp(_) => throw_unsupported("super property access"),
        MetaProp(_) => throw_unsupported("new.target / import.meta"),
        PrivateName(_) => throw_unsupported("private name (#field)"),
        Invalid(_) => throw_unsupported("invalid expression"),
        _ => throw_unsupported("expression form outside the interpreter subset"),
    }
}

// ── identifiers ────────────────────────────────────────────────────────────

fn eval_ident(ctx: &Ctx, name: &str, env_idx: usize) -> f64 {
    let _ = ctx;
    if let Some(v) = env::lookup(root_get(env_idx), name) {
        return v;
    }
    match name {
        "undefined" => return bridge::undefined(),
        "NaN" => return bridge::make_number(f64::NAN),
        "Infinity" => return bridge::make_number(f64::INFINITY),
        "globalThis" => return crate::object::js_get_global_this(),
        "arguments" => throw_unsupported("the arguments object"),
        _ => {}
    }
    let global = bridge::global_lookup(name);
    if !bridge::is_undefined(global) {
        return global;
    }
    if global_has_own(name) {
        return global;
    }
    bridge::throw_reference_error(&format!("{name} is not defined"))
}

fn global_has_own(name: &str) -> bool {
    let g = crate::object::js_get_global_this();
    let g_idx = root_push(g);
    let key = bridge::make_string(name);
    let has = crate::object::js_object_has_own(root_get(g_idx), key);
    roots_truncate(g_idx);
    bridge::truthy(has)
}

// ── literals ───────────────────────────────────────────────────────────────

fn eval_lit(lit: &ast::Lit) -> f64 {
    match lit {
        ast::Lit::Str(s) => bridge::make_string(&String::from_utf8_lossy(s.value.as_bytes())),
        ast::Lit::Num(n) => bridge::make_number(n.value),
        ast::Lit::Bool(b) => bridge::boolean(b.value),
        ast::Lit::Null(_) => bridge::null(),
        ast::Lit::Regex(r) => bridge::make_regex(&r.exp, &r.flags),
        ast::Lit::BigInt(_) => throw_unsupported("bigint literal"),
        ast::Lit::JSXText(_) => throw_unsupported("JSX"),
    }
}

fn eval_template(ctx: &Ctx, tpl: &ast::Tpl, env_idx: usize) -> f64 {
    // quasi0 expr0 quasi1 expr1 … quasiN — build via the runtime's dynamic
    // string concat so ToString semantics match compiled code exactly.
    let acc_idx = root_push(bridge::make_string(cooked_str(&tpl.quasis[0]).as_ref()));
    for (i, e) in tpl.exprs.iter().enumerate() {
        let v = eval_expr(ctx, e, env_idx);
        let v_idx = root_push(v);
        let s = bridge::to_string_value(root_get(v_idx));
        let joined = unsafe { crate::value::js_dynamic_string_or_number_add(root_get(acc_idx), s) };
        super::root_set(acc_idx, joined);
        roots_truncate(v_idx);
        let quasi = bridge::make_string(cooked_str(&tpl.quasis[i + 1]).as_ref());
        let joined = unsafe { crate::value::js_dynamic_string_or_number_add(root_get(acc_idx), quasi) };
        super::root_set(acc_idx, joined);
    }
    let result = root_get(acc_idx);
    roots_truncate(acc_idx);
    result
}

fn cooked_str(q: &ast::TplElement) -> String {
    match &q.cooked {
        Some(c) => String::from_utf8_lossy(c.as_bytes()).into_owned(),
        None => q.raw.to_string(),
    }
}

fn eval_array_lit(ctx: &Ctx, a: &ast::ArrayLit, env_idx: usize) -> f64 {
    let arr_idx = root_push(bridge::array_new());
    for elem in &a.elems {
        match elem {
            None => bridge::array_push_rooted(arr_idx, f64::from_bits(crate::value::TAG_HOLE)),
            Some(e) if e.spread.is_some() => {
                let src = eval_expr(ctx, &e.expr, env_idx);
                let src_idx = root_push(src);
                if !bridge::is_array_value(root_get(src_idx)) {
                    throw_unsupported("array spread of a non-array iterable");
                }
                let len = bridge::array_length(root_get(src_idx));
                for i in 0..len {
                    let v = bridge::array_get(root_get(src_idx), i);
                    bridge::array_push_rooted(arr_idx, v);
                }
                roots_truncate(src_idx);
            }
            Some(e) => {
                let v = eval_expr(ctx, &e.expr, env_idx);
                bridge::array_push_rooted(arr_idx, v);
            }
        }
    }
    let arr = root_get(arr_idx);
    roots_truncate(arr_idx);
    arr
}

fn eval_object_lit(ctx: &Ctx, o: &ast::ObjectLit, env_idx: usize) -> f64 {
    let obj_idx = root_push(bridge::object_new());
    for prop in &o.props {
        match prop {
            ast::PropOrSpread::Spread(s) => {
                // `{ ...src }` — copy own enumerable props (zod's issue-path
                // rewriting relies on this).
                let src = eval_expr(ctx, &s.expr, env_idx);
                if bridge::is_nullish(src) {
                    continue;
                }
                let src_idx = root_push(src);
                let keys = bridge::own_keys(root_get(src_idx));
                let keys_idx = root_push(keys);
                let len = bridge::array_length(root_get(keys_idx));
                for i in 0..len {
                    let key = bridge::array_get(root_get(keys_idx), i);
                    let key_idx = root_push(key);
                    let value = bridge::get_index(root_get(src_idx), root_get(key_idx));
                    let value_idx = root_push(value);
                    bridge::set_index(
                        root_get(obj_idx),
                        root_get(key_idx),
                        root_get(value_idx),
                    );
                    roots_truncate(key_idx);
                }
                roots_truncate(src_idx);
            }
            ast::PropOrSpread::Prop(p) => match p.as_ref() {
                ast::Prop::Shorthand(ident) => {
                    let value = eval_ident(ctx, &ident.sym, env_idx);
                    let value_idx = root_push(value);
                    bridge::set_member(root_get(obj_idx), &ident.sym, root_get(value_idx));
                    roots_truncate(value_idx);
                }
                ast::Prop::KeyValue(kv) => {
                    let value = eval_expr(ctx, &kv.value, env_idx);
                    set_prop_by_name(ctx, obj_idx, &kv.key, value, env_idx);
                }
                ast::Prop::Method(m) => {
                    if m.function.is_generator || m.function.is_async {
                        throw_unsupported("generator/async object method");
                    }
                    let value = make_function_value(
                        ctx,
                        m.function.params.iter().map(|p| p.pat.clone()).collect(),
                        InterpBody::Block(
                            m.function
                                .body
                                .as_ref()
                                .map(|b| b.stmts.clone())
                                .unwrap_or_default(),
                        ),
                        false,
                        None,
                        m.function.as_ref() as *const ast::Function as usize,
                        env_idx,
                    );
                    set_prop_by_name(ctx, obj_idx, &m.key, value, env_idx);
                }
                ast::Prop::Getter(_) | ast::Prop::Setter(_) => {
                    throw_unsupported("getter/setter in object literal")
                }
                ast::Prop::Assign(_) => throw_unsupported("invalid object literal property"),
            },
        }
    }
    let obj = root_get(obj_idx);
    roots_truncate(obj_idx);
    obj
}

fn set_prop_by_name(
    ctx: &Ctx,
    obj_idx: usize,
    key: &ast::PropName,
    value: f64,
    env_idx: usize,
) {
    let value_idx = root_push(value);
    match key {
        ast::PropName::Ident(i) => {
            bridge::set_member(root_get(obj_idx), &i.sym, root_get(value_idx))
        }
        ast::PropName::Str(s) => bridge::set_member(
            root_get(obj_idx),
            &String::from_utf8_lossy(s.value.as_bytes()),
            root_get(value_idx),
        ),
        ast::PropName::Num(n) => {
            let k = bridge::make_number(n.value);
            bridge::set_index(root_get(obj_idx), k, root_get(value_idx));
        }
        ast::PropName::Computed(c) => {
            let k = eval_expr(ctx, &c.expr, env_idx);
            let k_idx = root_push(k);
            bridge::set_index(root_get(obj_idx), root_get(k_idx), root_get(value_idx));
            roots_truncate(k_idx);
        }
        ast::PropName::BigInt(_) => throw_unsupported("bigint property key"),
    }
    roots_truncate(value_idx);
}

// ── unary / update ─────────────────────────────────────────────────────────

fn eval_unary(ctx: &Ctx, u: &ast::UnaryExpr, env_idx: usize) -> f64 {
    use ast::UnaryOp::*;
    match u.op {
        TypeOf => {
            // `typeof missingIdent` must not throw.
            if let ast::Expr::Ident(i) = u.arg.as_ref() {
                if !env::is_bound(root_get(env_idx), &i.sym) && !global_has_own(&i.sym) {
                    let special = matches!(&*i.sym, "undefined" | "NaN" | "Infinity" | "globalThis");
                    if !special {
                        return bridge::make_string("undefined");
                    }
                }
            }
            let v = eval_expr(ctx, &u.arg, env_idx);
            bridge::typeof_value(v)
        }
        Bang => {
            let v = eval_expr(ctx, &u.arg, env_idx);
            bridge::boolean(!bridge::truthy(v))
        }
        Minus => {
            let v = eval_expr(ctx, &u.arg, env_idx);
            let n = bridge::to_number(v);
            bridge::make_number(-n)
        }
        Plus => {
            let v = eval_expr(ctx, &u.arg, env_idx);
            bridge::make_number(bridge::to_number(v))
        }
        Tilde => {
            let v = eval_expr(ctx, &u.arg, env_idx);
            bridge::make_number(!bridge::to_int32(v) as f64)
        }
        Void => {
            let _ = eval_expr(ctx, &u.arg, env_idx);
            bridge::undefined()
        }
        Delete => match u.arg.as_ref() {
            ast::Expr::Member(m) => {
                let (obj_idx, key_idx) = eval_member_parts(ctx, m, env_idx);
                let deleted = crate::object::js_object_delete_dynamic_value(
                    root_get(obj_idx),
                    root_get(key_idx),
                );
                roots_truncate(obj_idx);
                bridge::boolean(deleted != 0)
            }
            _ => {
                let _ = eval_expr(ctx, &u.arg, env_idx);
                bridge::boolean(true)
            }
        },
    }
}

fn eval_update(ctx: &Ctx, u: &ast::UpdateExpr, env_idx: usize) -> f64 {
    let old = eval_expr(ctx, &u.arg, env_idx);
    let old_num = bridge::to_number(old);
    let delta = if u.op == ast::UpdateOp::PlusPlus {
        1.0
    } else {
        -1.0
    };
    let new_value = bridge::make_number(old_num + delta);
    assign_to_target_expr(ctx, &u.arg, new_value, env_idx);
    if u.prefix {
        bridge::make_number(old_num + delta)
    } else {
        bridge::make_number(old_num)
    }
}

// ── binary ─────────────────────────────────────────────────────────────────

fn eval_binary(ctx: &Ctx, b: &ast::BinExpr, env_idx: usize) -> f64 {
    use ast::BinaryOp::*;
    // Short-circuit forms first.
    match b.op {
        LogicalAnd => {
            let l = eval_expr(ctx, &b.left, env_idx);
            if !bridge::truthy(l) {
                return l;
            }
            return eval_expr(ctx, &b.right, env_idx);
        }
        LogicalOr => {
            let l = eval_expr(ctx, &b.left, env_idx);
            if bridge::truthy(l) {
                return l;
            }
            return eval_expr(ctx, &b.right, env_idx);
        }
        NullishCoalescing => {
            let l = eval_expr(ctx, &b.left, env_idx);
            if !bridge::is_nullish(l) {
                return l;
            }
            return eval_expr(ctx, &b.right, env_idx);
        }
        _ => {}
    }
    let l = eval_expr(ctx, &b.left, env_idx);
    let l_idx = root_push(l);
    let r = eval_expr(ctx, &b.right, env_idx);
    let r_idx = root_push(r);
    let l = root_get(l_idx);
    let r = root_get(r_idx);
    let result = match b.op {
        Add => unsafe { crate::value::js_dynamic_string_or_number_add(l, r) },
        Sub => unsafe { crate::value::js_dynamic_sub(l, r) },
        Mul => unsafe { crate::value::js_dynamic_mul(l, r) },
        Div => unsafe { crate::value::js_dynamic_div(l, r) },
        Mod => unsafe { crate::value::js_dynamic_mod(l, r) },
        Exp => unsafe { crate::value::js_dynamic_pow(l, r) },
        BitAnd => unsafe { crate::value::js_dynamic_bitand(l, r) },
        BitOr => unsafe { crate::value::js_dynamic_bitor(l, r) },
        BitXor => unsafe { crate::value::js_dynamic_bitxor(l, r) },
        LShift => unsafe { crate::value::js_dynamic_shl(l, r) },
        RShift => unsafe { crate::value::js_dynamic_shr(l, r) },
        ZeroFillRShift => unsafe { crate::value::js_dynamic_ushr(l, r) },
        EqEq => bridge::boolean(bridge::loose_equals(l, r)),
        NotEq => bridge::boolean(!bridge::loose_equals(l, r)),
        EqEqEq => bridge::boolean(bridge::strict_equals(l, r)),
        NotEqEq => bridge::boolean(!bridge::strict_equals(l, r)),
        Lt => bridge::boolean(bridge::compare(l, r) == -1),
        LtEq => bridge::boolean(matches!(bridge::compare(l, r), -1 | 0)),
        Gt => bridge::boolean(bridge::compare(l, r) == 1),
        GtEq => bridge::boolean(matches!(bridge::compare(l, r), 0 | 1)),
        In => bridge::boolean(bridge::in_operator(l, r)),
        InstanceOf => bridge::boolean(bridge::instanceof(l, r)),
        LogicalAnd | LogicalOr | NullishCoalescing => unreachable!(),
    };
    roots_truncate(l_idx);
    result
}

// ── member access ──────────────────────────────────────────────────────────

/// Evaluate a member expression's object + key; returns rooted indices
/// (object, key). Caller truncates from the object index.
fn eval_member_parts(ctx: &Ctx, m: &ast::MemberExpr, env_idx: usize) -> (usize, usize) {
    let obj = eval_expr(ctx, &m.obj, env_idx);
    let obj_idx = root_push(obj);
    let key = match &m.prop {
        ast::MemberProp::Ident(i) => bridge::make_string(&i.sym),
        ast::MemberProp::Computed(c) => eval_expr(ctx, &c.expr, env_idx),
        ast::MemberProp::PrivateName(_) => throw_unsupported("private member access"),
    };
    let key_idx = root_push(key);
    (obj_idx, key_idx)
}

fn eval_member(ctx: &Ctx, m: &ast::MemberExpr, env_idx: usize) -> f64 {
    let obj = eval_expr(ctx, &m.obj, env_idx);
    match &m.prop {
        ast::MemberProp::Ident(i) => bridge::get_member(obj, &i.sym),
        ast::MemberProp::Computed(c) => {
            let obj_idx = root_push(obj);
            let key = eval_expr(ctx, &c.expr, env_idx);
            let key_idx = root_push(key);
            let value = bridge::get_index(root_get(obj_idx), root_get(key_idx));
            roots_truncate(obj_idx);
            let _ = key_idx;
            value
        }
        ast::MemberProp::PrivateName(_) => throw_unsupported("private member access"),
    }
}

fn eval_opt_chain(ctx: &Ctx, o: &ast::OptChainExpr, env_idx: usize) -> f64 {
    match o.base.as_ref() {
        ast::OptChainBase::Member(m) => {
            let obj = eval_expr(ctx, &m.obj, env_idx);
            if bridge::is_nullish(obj) {
                return bridge::undefined();
            }
            match &m.prop {
                ast::MemberProp::Ident(i) => bridge::get_member(obj, &i.sym),
                ast::MemberProp::Computed(c) => {
                    let obj_idx = root_push(obj);
                    let key = eval_expr(ctx, &c.expr, env_idx);
                    let value = bridge::get_index(root_get(obj_idx), key);
                    roots_truncate(obj_idx);
                    value
                }
                ast::MemberProp::PrivateName(_) => throw_unsupported("private member access"),
            }
        }
        ast::OptChainBase::Call(call) => {
            let callee = eval_expr(ctx, &call.callee, env_idx);
            if bridge::is_nullish(callee) {
                return bridge::undefined();
            }
            let callee_idx = root_push(callee);
            let args = eval_args(ctx, &call.args, env_idx);
            let result = call_with_args(root_get(callee_idx), bridge::undefined(), &args);
            roots_truncate(callee_idx);
            result
        }
    }
}

// ── assignment ─────────────────────────────────────────────────────────────

fn eval_assign(ctx: &Ctx, a: &ast::AssignExpr, env_idx: usize) -> f64 {
    use ast::AssignOp;
    // Logical assignment short-circuits before evaluating the RHS.
    if matches!(
        a.op,
        AssignOp::AndAssign | AssignOp::OrAssign | AssignOp::NullishAssign
    ) {
        let current = eval_assign_target_read(ctx, &a.left, env_idx);
        let should_assign = match a.op {
            AssignOp::AndAssign => bridge::truthy(current),
            AssignOp::OrAssign => !bridge::truthy(current),
            AssignOp::NullishAssign => bridge::is_nullish(current),
            _ => unreachable!(),
        };
        if !should_assign {
            return current;
        }
        let rhs = eval_expr(ctx, &a.right, env_idx);
        let rhs_idx = root_push(rhs);
        assign_to_assign_target(ctx, &a.left, root_get(rhs_idx), env_idx);
        let rhs = root_get(rhs_idx);
        roots_truncate(rhs_idx);
        return rhs;
    }
    if a.op == AssignOp::Assign {
        let rhs = eval_expr(ctx, &a.right, env_idx);
        let rhs_idx = root_push(rhs);
        assign_to_assign_target(ctx, &a.left, root_get(rhs_idx), env_idx);
        let rhs = root_get(rhs_idx);
        roots_truncate(rhs_idx);
        return rhs;
    }
    // Compound: read-modify-write.
    let current = eval_assign_target_read(ctx, &a.left, env_idx);
    let cur_idx = root_push(current);
    let rhs = eval_expr(ctx, &a.right, env_idx);
    let rhs_idx = root_push(rhs);
    let l = root_get(cur_idx);
    let r = root_get(rhs_idx);
    let combined = match a.op {
        AssignOp::AddAssign => unsafe { crate::value::js_dynamic_string_or_number_add(l, r) },
        AssignOp::SubAssign => unsafe { crate::value::js_dynamic_sub(l, r) },
        AssignOp::MulAssign => unsafe { crate::value::js_dynamic_mul(l, r) },
        AssignOp::DivAssign => unsafe { crate::value::js_dynamic_div(l, r) },
        AssignOp::ModAssign => unsafe { crate::value::js_dynamic_mod(l, r) },
        AssignOp::ExpAssign => unsafe { crate::value::js_dynamic_pow(l, r) },
        AssignOp::BitAndAssign => unsafe { crate::value::js_dynamic_bitand(l, r) },
        AssignOp::BitOrAssign => unsafe { crate::value::js_dynamic_bitor(l, r) },
        AssignOp::BitXorAssign => unsafe { crate::value::js_dynamic_bitxor(l, r) },
        AssignOp::LShiftAssign => unsafe { crate::value::js_dynamic_shl(l, r) },
        AssignOp::RShiftAssign => unsafe { crate::value::js_dynamic_shr(l, r) },
        AssignOp::ZeroFillRShiftAssign => unsafe { crate::value::js_dynamic_ushr(l, r) },
        _ => unreachable!(),
    };
    let combined_idx = root_push(combined);
    assign_to_assign_target(ctx, &a.left, root_get(combined_idx), env_idx);
    let combined = root_get(combined_idx);
    roots_truncate(cur_idx);
    combined
}

fn eval_assign_target_read(ctx: &Ctx, target: &ast::AssignTarget, env_idx: usize) -> f64 {
    match target {
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(b)) => {
            eval_ident(ctx, &b.id.sym, env_idx)
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::Member(m)) => {
            eval_member(ctx, m, env_idx)
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::Paren(p)) => {
            eval_expr(ctx, &p.expr, env_idx)
        }
        _ => throw_unsupported("compound assignment to this target"),
    }
}

fn assign_to_assign_target(
    ctx: &Ctx,
    target: &ast::AssignTarget,
    value: f64,
    env_idx: usize,
) {
    match target {
        ast::AssignTarget::Simple(simple) => match simple {
            ast::SimpleAssignTarget::Ident(b) => {
                env::assign(root_get(env_idx), &b.id.sym, value);
            }
            ast::SimpleAssignTarget::Member(m) => assign_to_member(ctx, m, value, env_idx),
            ast::SimpleAssignTarget::Paren(p) => {
                assign_to_target_expr(ctx, &p.expr, value, env_idx)
            }
            _ => throw_unsupported("assignment to this target form"),
        },
        ast::AssignTarget::Pat(pat) => match pat {
            ast::AssignTargetPat::Array(a) => {
                bind_pattern(ctx, &ast::Pat::Array(a.clone()), value, env_idx, false)
            }
            ast::AssignTargetPat::Object(o) => {
                bind_pattern(ctx, &ast::Pat::Object(o.clone()), value, env_idx, false)
            }
            ast::AssignTargetPat::Invalid(_) => throw_unsupported("invalid assignment pattern"),
        },
    }
}

fn assign_to_member(ctx: &Ctx, m: &ast::MemberExpr, value: f64, env_idx: usize) {
    let value_idx = root_push(value);
    let (obj_idx, key_idx) = eval_member_parts(ctx, m, env_idx);
    bridge::set_index(root_get(obj_idx), root_get(key_idx), root_get(value_idx));
    roots_truncate(value_idx);
}

/// Assignment to an expression target (update expressions, `Pat::Expr`
/// destructuring targets, parenthesized targets).
pub(crate) fn assign_to_target_expr(ctx: &Ctx, e: &ast::Expr, value: f64, env_idx: usize) {
    match e {
        ast::Expr::Ident(i) => env::assign(root_get(env_idx), &i.sym, value),
        ast::Expr::Member(m) => assign_to_member(ctx, m, value, env_idx),
        ast::Expr::Paren(p) => assign_to_target_expr(ctx, &p.expr, value, env_idx),
        _ => throw_unsupported("assignment to this expression form"),
    }
}

// ── calls ──────────────────────────────────────────────────────────────────

fn eval_args(ctx: &Ctx, args: &[ast::ExprOrSpread], env_idx: usize) -> Vec<f64> {
    // Evaluate every argument into rooted slots, then snapshot right before
    // the call (no allocation between the snapshot reads and the dispatch).
    let base = super::roots_len();
    let mut idxs = Vec::with_capacity(args.len());
    for a in args {
        if a.spread.is_some() {
            // Spread-call: flatten array spreads inline.
            let src = eval_expr(ctx, &a.expr, env_idx);
            let src_idx = root_push(src);
            if !bridge::is_array_value(root_get(src_idx)) {
                throw_unsupported("spread call argument that is not an array");
            }
            let len = bridge::array_length(root_get(src_idx));
            for i in 0..len {
                let v = bridge::array_get(root_get(src_idx), i);
                idxs.push(root_push(v));
            }
        } else {
            let v = eval_expr(ctx, &a.expr, env_idx);
            idxs.push(root_push(v));
        }
    }
    let snapshot: Vec<f64> = idxs.iter().map(|&i| root_get(i)).collect();
    let _ = base;
    snapshot
}

fn call_with_args(callee: f64, this: f64, args: &[f64]) -> f64 {
    bridge::call_function(callee, this, args)
}

fn eval_call(ctx: &Ctx, c: &ast::CallExpr, env_idx: usize) -> f64 {
    let callee = match &c.callee {
        ast::Callee::Expr(e) => e,
        ast::Callee::Super(_) => throw_unsupported("super(…) call"),
        ast::Callee::Import(_) => throw_unsupported("dynamic import in interpreted code"),
    };
    match callee.as_ref() {
        // `obj.m(args)` / `obj[k](args)`: route through the runtime's method
        // dispatch so builtin prototypes work and `this` binds to the
        // receiver — for host closures AND interpreted closures alike.
        ast::Expr::Member(m) => {
            let obj = eval_expr(ctx, &m.obj, env_idx);
            let obj_idx = root_push(obj);
            match &m.prop {
                ast::MemberProp::Ident(i) => {
                    let name = i.sym.to_string();
                    if bridge::is_nullish(root_get(obj_idx)) {
                        bridge::throw_type_error(&format!(
                            "Cannot read properties of {} (reading '{name}')",
                            if bridge::is_undefined(root_get(obj_idx)) {
                                "undefined"
                            } else {
                                "null"
                            }
                        ));
                    }
                    let args = eval_args(ctx, &c.args, env_idx);
                    let result = bridge::call_method(root_get(obj_idx), &name, &args);
                    roots_truncate(obj_idx);
                    result
                }
                ast::MemberProp::Computed(comp) => {
                    let key = eval_expr(ctx, &comp.expr, env_idx);
                    let key_idx = root_push(key);
                    let args = eval_args(ctx, &c.args, env_idx);
                    let result =
                        bridge::call_method_value(root_get(obj_idx), root_get(key_idx), &args);
                    roots_truncate(obj_idx);
                    result
                }
                ast::MemberProp::PrivateName(_) => throw_unsupported("private method call"),
            }
        }
        // Plain call: resolve the callee value, dispatch with this=undefined.
        other => {
            let callee_value = eval_call_callee(ctx, other, env_idx);
            let callee_idx = root_push(callee_value);
            let args = eval_args(ctx, &c.args, env_idx);
            let result = call_with_args(root_get(callee_idx), bridge::undefined(), &args);
            roots_truncate(callee_idx);
            result
        }
    }
}

/// Resolve a non-member callee. Identifier callees get targeted fallbacks for
/// a few universal globals in case the compiled binary's `globalThis` bag
/// doesn't carry them as callable values.
fn eval_call_callee(ctx: &Ctx, e: &ast::Expr, env_idx: usize) -> f64 {
    if let ast::Expr::Ident(i) = e {
        let name: &str = &i.sym;
        if let Some(v) = env::lookup(root_get(env_idx), name) {
            return v;
        }
        let global = bridge::global_lookup(name);
        if !bridge::is_undefined(global) {
            return global;
        }
        // The generated-code corpus leans on these; if the global bag lacks
        // them as first-class closures, give the interpreter native
        // equivalents rather than failing the whole validator.
        match name {
            "isNaN" | "isFinite" | "String" | "Number" | "Boolean" => {
                bridge::throw_reference_error(&format!(
                    "{name} is not available as a callable global in this binary \
                     (perry runtime interpreter, #6559)"
                ))
            }
            _ => bridge::throw_reference_error(&format!("{name} is not defined")),
        }
    } else {
        eval_expr(ctx, e, env_idx)
    }
}

// ── new ────────────────────────────────────────────────────────────────────

fn eval_new(ctx: &Ctx, n: &ast::NewExpr, env_idx: usize) -> f64 {
    let args_ast: &[ast::ExprOrSpread] = n.args.as_deref().unwrap_or(&[]);

    // Builtin error constructors + RegExp get direct runtime paths — the
    // generated code throws `new TypeError(…)` / `new Error(…)` and builds
    // `new RegExp(p, f)`, and these must work even if the compiled binary
    // never referenced the constructors itself.
    if let ast::Expr::Ident(i) = n.callee.as_ref() {
        let name: &str = &i.sym;
        if !env::is_bound(root_get(env_idx), name) {
            let kind = match name {
                "Error" => Some(crate::error::ERROR_KIND_ERROR),
                "TypeError" => Some(crate::error::ERROR_KIND_TYPE_ERROR),
                "RangeError" => Some(crate::error::ERROR_KIND_RANGE_ERROR),
                "SyntaxError" => Some(crate::error::ERROR_KIND_SYNTAX_ERROR),
                "ReferenceError" => Some(crate::error::ERROR_KIND_REFERENCE_ERROR),
                _ => None,
            };
            if let Some(kind) = kind {
                let args = eval_args(ctx, args_ast, env_idx);
                let msg_value = args.first().copied().unwrap_or(bridge::undefined());
                let msg_idx = root_push(msg_value);
                let msg_str = if bridge::is_undefined(root_get(msg_idx)) {
                    String::new()
                } else {
                    bridge::to_rust_string(root_get(msg_idx))
                };
                let msg =
                    crate::string::js_string_from_bytes(msg_str.as_ptr(), msg_str.len() as u32);
                let err = crate::error::js_error_new_kind_with_options(
                    kind,
                    msg,
                    bridge::undefined(),
                );
                roots_truncate(msg_idx);
                return crate::value::js_nanbox_pointer(err as i64);
            }
            if name == "RegExp" {
                let args = eval_args(ctx, args_ast, env_idx);
                let pattern = args
                    .first()
                    .map(|v| bridge::to_rust_string(*v))
                    .unwrap_or_default();
                let flags = args
                    .get(1)
                    .filter(|v| !bridge::is_undefined(**v))
                    .map(|v| bridge::to_rust_string(*v))
                    .unwrap_or_default();
                return bridge::make_regex(&pattern, &flags);
            }
        }
    }

    // General case: `new <host value>(…)` — find-my-way's `new NullObject()`
    // (the constructor arrives as a Function parameter) and any other host
    // class the generated code was handed.
    let callee = eval_expr(ctx, &n.callee, env_idx);
    let callee_idx = root_push(callee);
    let args = eval_args(ctx, args_ast, env_idx);
    let result = bridge::construct(root_get(callee_idx), &args);
    roots_truncate(callee_idx);
    result
}
