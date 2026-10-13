//! Typed bodies for closures (fninfer slice 0, owner decision 140).
//!
//! A closure whose parameter types are concrete — declared, or inferred by
//! fninfer and written into the HIR as guarded annotations
//! (`PERRY_FNINFER_APPLY`) — gets a second compile of its ordinary body,
//! `{public}$tbody`, in which those parameter types are PROOFS
//! (`FnCtx::proven_local_types`) instead of hints. The public body symbol
//! becomes a dispatcher with the unchanged JS body ABI: one O(1) entry check
//! of the arguments routes the call to the typed body, everything else to the
//! unchanged generic body (`{public}$generic`). Both are ordinary compiled code
//! of the same function, so this is not a tier: which body runs is decided by
//! the argument values of each call, never by a profile.
//!
//! The typed body shares everything else with the generic one: captures are
//! read from the same closure environment (`%this_closure`), `this` comes from
//! the same receiver parameter, and `arguments`-using, generator and
//! suspending async bodies are not cloned at all.
//!
//! Entry check per parameter (all branch-free, in the entry block):
//! - `Number`: the two-tier test of `typed_entry.rs` (plain double, or an
//!   int32 box converted to the equal double);
//! - `String`: the heap/short string tag;
//! - `Boolean`: the two boolean tags;
//! - `Named(C)` for a class with a class id: the fused receiver range test
//!   and the header's class id (`ObjectHeader` word 0, low half) — the load
//!   goes to the module's zero guard word when the value is not an object.
//!
//! A reference proof (a class instance) holds only where nothing can reach
//! the object: the same #8094 rule spec_abi applies — a body that calls
//! anything, or writes through the parameter, keeps the class as a hint, and
//! its sites keep their own shape checks.
//!
//! Clone admission is structural: the clone is emitted only when the body has
//! at least one site whose lowering the proof changes (an operation on the
//! parameter), and the dispatcher is installed only when the typed body
//! actually has fewer runtime entry calls than the generic body
//! ([`runtime_call_sites`]). Otherwise the clone is removed again.

use std::collections::{HashMap, HashSet};

use perry_hir::types::Type;
use perry_hir::{Expr, Param, Stmt};

use crate::function::LlFunction;
use crate::module::LlModule;
use crate::types::{DOUBLE, I1, I32, I64};

/// `PERRY_TYPED_BODIES=1` turns typed closure bodies on. Off by default while
/// measured; keyed into the object cache.
pub(crate) fn typed_bodies_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        matches!(
            std::env::var("PERRY_TYPED_BODIES").as_deref(),
            Ok("1") | Ok("on") | Ok("true")
        )
    })
}

/// The entry check for one parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BodyGuard {
    None,
    Number,
    String,
    Boolean,
    Class { class_id: u32 },
}

/// A closure's typed-body plan.
#[derive(Debug, Clone)]
pub(crate) struct TypedBodyPlan {
    pub guards: Vec<BodyGuard>,
    /// Facts the typed body may rely on (param LocalId → type).
    pub proofs: HashMap<u32, Type>,
    /// Sites whose lowering the proofs change (the admission count).
    pub sites: usize,
}

pub(crate) fn typed_body_name(public: &str) -> String {
    format!("{public}$tbody")
}

pub(crate) fn generic_body_name(public: &str) -> String {
    format!("{public}$generic")
}

fn guard_for(ty: &Type, class_ids: &HashMap<String, u32>) -> Option<(BodyGuard, Type)> {
    match ty {
        Type::Number | Type::Int32 => Some((BodyGuard::Number, Type::Number)),
        Type::String => Some((BodyGuard::String, Type::String)),
        Type::Boolean => Some((BodyGuard::Boolean, Type::Boolean)),
        Type::Named(c) => class_ids
            .get(c)
            .filter(|id| **id != 0)
            .map(|id| (BodyGuard::Class { class_id: *id }, ty.clone())),
        _ => None,
    }
}

/// Count the sites of `body` (closure bodies excluded) whose lowering a proof
/// of `ty` for `id` changes: operators on the parameter for primitives,
/// member reads/writes/calls on it for strings and class instances.
fn proof_sites(body: &[Stmt], id: u32, ty: &Type) -> usize {
    let mut n = 0usize;
    let is_p = |e: &Expr| matches!(e, Expr::LocalGet(l) if *l == id);
    let mut visit = |e: &Expr| match e {
        Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. }
            if is_p(left) || is_p(right) =>
        {
            n += 1
        }
        Expr::Unary { operand, .. } if is_p(operand) => n += 1,
        Expr::PropertyGet { object, .. } | Expr::PropertySet { object, .. }
            if is_p(object) && matches!(ty, Type::String | Type::Named(_)) =>
        {
            n += 1
        }
        Expr::IndexGet { object, .. } if is_p(object) && matches!(ty, Type::String) => n += 1,
        _ => {}
    };
    fn walk(e: &Expr, visit: &mut dyn FnMut(&Expr)) {
        visit(e);
        if matches!(e, Expr::Closure { .. }) {
            return;
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| walk(c, visit));
    }
    for s in body {
        perry_hir::walker::stmt_any_expr(s, &mut |e| {
            walk(e, &mut visit);
            false
        });
    }
    n
}

/// Plan the typed body of the closure `(params, body)`, or `None`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn closure_typed_body_plan(
    params: &[Param],
    body: &[Stmt],
    is_async: bool,
    is_generator: bool,
    synthetic_arguments: bool,
    boxed_vars: &HashSet<u32>,
    reassigned: &HashSet<u32>,
    class_ids: &HashMap<String, u32>,
) -> Option<TypedBodyPlan> {
    if is_generator || (is_async && super::param_guard::body_contains_await(body)) {
        return None;
    }
    if synthetic_arguments
        || params
            .iter()
            .any(|p| p.is_rest || p.arguments_object.is_some())
    {
        return None;
    }
    let body_calls = crate::collectors::body_contains_call(body);
    let mut guards = Vec::with_capacity(params.len());
    let mut proofs = HashMap::new();
    let mut sites = 0usize;
    for p in params {
        let stable = !boxed_vars.contains(&p.id)
            && !reassigned.contains(&p.id)
            && !crate::collectors::has_any_mutation(body, p.id);
        let Some((guard, proof)) = guard_for(&p.ty, class_ids).filter(|_| stable) else {
            guards.push(BodyGuard::None);
            continue;
        };
        // #8094: a class proof does not survive a call or a write through
        // the parameter; such a body keeps its own per-site checks.
        if matches!(guard, BodyGuard::Class { .. }) && body_calls {
            guards.push(BodyGuard::None);
            continue;
        }
        sites += proof_sites(body, p.id, &proof);
        proofs.insert(p.id, proof);
        guards.push(guard);
    }
    if proofs.is_empty() || sites == 0 {
        return None;
    }
    Some(TypedBodyPlan {
        guards,
        proofs,
        sites,
    })
}

/// Runtime entries a body calls: every direct call to a runtime symbol
/// (`js_*`, `perry_*` other than compiled bodies), excluding GC bookkeeping.
/// The clone is worth installing only when this number drops.
pub(crate) fn runtime_call_sites(f: &LlFunction) -> usize {
    let mut n = 0usize;
    for b in f.blocks() {
        for inst in b.insts() {
            if let Some(callee) = inst.direct_callee() {
                let c = callee.trim_start_matches('@');
                let runtime = c.starts_with("js_") || c.starts_with("perry_rt_");
                let bookkeeping = c.starts_with("js_shadow_slot")
                    || c.starts_with("js_gc_")
                    || c.contains("statepoint");
                if runtime && !bookkeeping {
                    n += 1;
                }
            }
        }
    }
    n
}

/// Emit the public dispatcher `public(callee, this, args…)`: the entry check,
/// then the typed body or the generic body with the same arguments.
pub(crate) fn emit_typed_body_dispatch(
    llmod: &mut LlModule,
    public: &str,
    generic: &str,
    tbody: &str,
    param_ids: &[u32],
    guards: &[BodyGuard],
) {
    use crate::expr::body_call::{
        emit_js_body_call, js_body_params, JsBody, JS_BODY_CALLEE, JS_BODY_THIS,
    };
    let arg_names: Vec<String> = param_ids.iter().map(|id| format!("%arg{id}")).collect();
    let null_guard = format!("@{}", llmod.null_guard_global());
    let wf = llmod.define_function(public, DOUBLE, js_body_params(arg_names.iter().cloned()));
    wf.no_inline = false;
    let _ = wf.create_block("entry");
    let entry_guards: Vec<super::typed_entry::EntryArgGuard> = {
        let blk = wf.block_mut(0).unwrap();
        arg_names
            .iter()
            .zip(guards)
            .map(|(arg, g)| match g {
                BodyGuard::None => super::typed_entry::EntryArgGuard::default(),
                BodyGuard::Number => super::typed_entry::EntryArgGuard {
                    number: true,
                    exact: None,
                },
                BodyGuard::String => super::typed_entry::EntryArgGuard {
                    number: false,
                    exact: Some(super::typed_abi::emit_typed_string_guard(blk, arg)),
                },
                BodyGuard::Boolean => super::typed_entry::EntryArgGuard {
                    number: false,
                    exact: Some(super::typed_abi::emit_typed_i1_guard(blk, arg)),
                },
                BodyGuard::Class { class_id } => {
                    let bits = blk.bitcast_double_to_i64(arg);
                    let recv = crate::expr::receiver_range::emit_fused_receiver_test(blk, &bits);
                    let handle = crate::expr::receiver_range::emit_handle(blk, &recv.biased);
                    let zero = blk.ptrtoint(&null_guard, I64);
                    let addr = blk.select(I1, &recv.is_object_pointer, I64, &handle, &zero);
                    let ptr = blk.inttoptr(I64, &addr);
                    let cid = blk.load(I32, &ptr);
                    let same = blk.icmp_eq(I32, &cid, &class_id.to_string());
                    let ok = blk.and(I1, &recv.is_object_pointer, &same);
                    super::typed_entry::EntryArgGuard {
                        number: false,
                        exact: Some(ok),
                    }
                }
            })
            .collect()
    };
    super::typed_entry::emit_tiered_entry_dispatch(
        wf,
        "typed_body",
        &arg_names,
        &entry_guards,
        None,
        &mut |blk, values| {
            emit_js_body_call(
                blk,
                JsBody::Symbol(tbody),
                JS_BODY_CALLEE,
                JS_BODY_THIS,
                values,
            )
        },
        &mut |blk| {
            emit_js_body_call(
                blk,
                JsBody::Symbol(generic),
                JS_BODY_CALLEE,
                JS_BODY_THIS,
                &arg_names,
            )
        },
    );
}

/// [`closure_typed_body_plan`] for an `Expr::Closure`.
pub(crate) fn plan_for_closure_expr(
    closure_expr: &Expr,
    synthetic_arguments: bool,
    boxed_vars: &HashSet<u32>,
    reassigned: &HashSet<u32>,
    class_ids: &HashMap<String, u32>,
) -> Option<TypedBodyPlan> {
    let Expr::Closure {
        params,
        body,
        is_async,
        is_generator,
        ..
    } = closure_expr
    else {
        return None;
    };
    closure_typed_body_plan(
        params,
        body,
        *is_async,
        *is_generator,
        synthetic_arguments,
        boxed_vars,
        reassigned,
        class_ids,
    )
}

/// After the generic body (defined at `generic_index` under the public
/// symbol) and the typed body (from `tbody_index`) exist: install the
/// dispatcher when the typed body calls fewer runtime entries. Otherwise the
/// typed body is removed again.
pub(crate) fn install_closure_dispatch(
    llmod: &mut LlModule,
    generic_index: usize,
    tbody_index: usize,
    closure_expr: &Expr,
    plan: &TypedBodyPlan,
) {
    let Expr::Closure { params, .. } = closure_expr else {
        return;
    };
    let Some(public) = llmod.function_mut(generic_index).map(|f| f.name.clone()) else {
        return;
    };
    let tbody = typed_body_name(&public);
    let tb_ok = llmod
        .function_mut(tbody_index)
        .is_some_and(|f| f.name == tbody);
    if !tb_ok {
        return;
    }
    let gen_calls = llmod
        .function_mut(generic_index)
        .map(|f| runtime_call_sites(f))
        .unwrap_or(0);
    let tb_calls = llmod
        .function_mut(tbody_index)
        .map(|f| runtime_call_sites(f))
        .unwrap_or(0);
    let installed = tb_calls < gen_calls;
    log_clone(&public, plan, gen_calls, tb_calls, installed);
    if !installed {
        // Nothing references it; codegen-unit splitting would otherwise keep
        // the dead body in `.text`.
        llmod.remove_defined_function(tbody_index, &tbody);
        return;
    }
    let generic = generic_body_name(&public);
    llmod.rename_defined_function(generic_index, &generic);
    if let Some(f) = llmod.function_mut(generic_index) {
        f.linkage = "internal".to_string();
    }
    let ids: Vec<u32> = params.iter().map(|p| p.id).collect();
    emit_typed_body_dispatch(llmod, &public, &generic, &tbody, &ids, &plan.guards);
}

/// `PERRY_TYPED_BODIES_LOG=<path>`: one line per planned clone
/// (`symbol sites generic_calls typed_calls installed guards`).
fn log_clone(
    public: &str,
    plan: &TypedBodyPlan,
    gen_calls: usize,
    tb_calls: usize,
    installed: bool,
) {
    use std::io::Write;
    use std::sync::Mutex;
    static LOCK: Mutex<()> = Mutex::new(());
    let Some(path) = std::env::var_os("PERRY_TYPED_BODIES_LOG") else {
        return;
    };
    let _g = LOCK.lock();
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(
            f,
            "TB\t{public}\t{}\t{gen_calls}\t{tb_calls}\t{}\t{:?}",
            plan.sites,
            u8::from(installed),
            plan.guards
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::types::Type;
    use perry_hir::{BinaryOp, Expr, Param, Stmt};

    fn param(id: u32, ty: Type) -> Param {
        Param {
            id,
            name: format!("p{id}"),
            ty,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }
    }

    fn add_body(id: u32) -> Vec<Stmt> {
        vec![Stmt::Return(Some(Expr::Binary {
            op: BinaryOp::Mul,
            left: Box::new(Expr::LocalGet(id)),
            right: Box::new(Expr::Number(2.0)),
        }))]
    }

    fn plan(
        params: &[Param],
        body: &[Stmt],
        is_async: bool,
        is_gen: bool,
    ) -> Option<TypedBodyPlan> {
        closure_typed_body_plan(
            params,
            body,
            is_async,
            is_gen,
            false,
            &HashSet::new(),
            &HashSet::new(),
            &HashMap::new(),
        )
    }

    #[test]
    fn number_param_with_an_operator_site_is_planned() {
        let p = plan(&[param(1, Type::Number)], &add_body(1), false, false).expect("plan");
        assert_eq!(p.guards, vec![BodyGuard::Number]);
        assert_eq!(p.sites, 1);
        assert_eq!(p.proofs.get(&1), Some(&Type::Number));
    }

    #[test]
    fn untyped_or_unused_params_are_not_cloned() {
        assert!(plan(&[param(1, Type::Any)], &add_body(1), false, false).is_none());
        assert!(plan(&[param(1, Type::Number)], &add_body(2), false, false).is_none());
    }

    #[test]
    fn generators_rest_and_reassigned_params_are_not_cloned() {
        assert!(plan(&[param(1, Type::Number)], &add_body(1), false, true).is_none());
        let mut rest = param(1, Type::Number);
        rest.is_rest = true;
        assert!(plan(&[rest], &add_body(1), false, false).is_none());
        let mut body = add_body(1);
        body.insert(
            0,
            Stmt::Expr(Expr::LocalSet(1, Box::new(Expr::String("s".into())))),
        );
        assert!(plan(&[param(1, Type::Number)], &body, false, false).is_none());
    }

    #[test]
    fn a_class_proof_does_not_survive_a_call() {
        let mut ids = HashMap::new();
        ids.insert("C".to_string(), 7u32);
        let body = vec![
            Stmt::Expr(Expr::Call {
                callee: Box::new(Expr::LocalGet(9)),
                args: Vec::new(),
                type_args: Vec::new(),
                byte_offset: 0,
            }),
            Stmt::Return(Some(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(1)),
                property: "x".into(),
                byte_offset: 0,
            })),
        ];
        let p = closure_typed_body_plan(
            &[param(1, Type::Named("C".into()))],
            &body,
            false,
            false,
            false,
            &HashSet::new(),
            &HashSet::new(),
            &ids,
        );
        assert!(p.is_none());
        let no_call = vec![body[1].clone()];
        let p = closure_typed_body_plan(
            &[param(1, Type::Named("C".into()))],
            &no_call,
            false,
            false,
            false,
            &HashSet::new(),
            &HashSet::new(),
            &ids,
        )
        .expect("plan");
        assert_eq!(p.guards, vec![BodyGuard::Class { class_id: 7 }]);
    }
}
