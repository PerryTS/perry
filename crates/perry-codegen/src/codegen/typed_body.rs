//! Typed bodies for closures (fninfer slice 0, owner decision 140; Closed
//! World step 5b).
//!
//! Step 5b: a typed body exists to carry receiver proofs to field sites. A
//! parameter whose fninfer type is an ES class, or a union of up to four, is
//! entry-checked against that class-id set; inside the typed body it answers
//! `wpfacts_access` like a whole-program receiver fact, so its field sites
//! compile to §3.7 case 1 (one load or store) or case 2 (a select over the
//! class ids, each arm a fixed offset). The field facts (F1', F3: a definite
//! birth field of a prefix-stable class) hold for the object's whole life
//! and the parameter is never reassigned, so the proof survives calls in the
//! body (the #8094 rule is about `proven_local_types`, which this does not
//! use). A clone is planned only where such a site exists and installed only
//! where it carries more elided sites, and fewer runtime-entry calls, than
//! the generic body.
//!
//! A closure whose class parameters are typed — declared, or inferred by
//! fninfer (`ElisionFacts::receiver_params`, computed by the driver over the
//! final HIR when the program has a prefix-stable class) — gets a second compile of its ordinary body,
//! `{public}$tbody`, in which those class sets are receiver PROOFS
//! (`TypedBodyPlan::receiver_facts`) instead of hints. The public body symbol
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
//! Entry check per class parameter (branch-free, in the entry block): the
//! fused receiver range test and the header's class id (`ObjectHeader` word
//! 0, low half) against the set; the load goes to the module's zero guard
//! word when the value is not an object. Other parameters are not checked.

use std::collections::{HashMap, HashSet};

use perry_hir::types::Type;
use perry_hir::{Expr, Param, Stmt};

use crate::function::LlFunction;
use crate::module::LlModule;
use crate::types::{DOUBLE, I1, I32, I64};

/// Typed closure bodies, on by default (step 5b); `PERRY_TYPED_BODIES=0`
/// turns them off (A/B control). Keyed into the object cache.
pub(crate) fn typed_bodies_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_TYPED_BODIES").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// The entry check for one parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BodyGuard {
    None,
    /// The header's class id is one of these (ES classes, at most four).
    Class {
        class_ids: Vec<u32>,
    },
}

/// A closure's typed-body plan.
#[derive(Debug, Clone)]
pub(crate) struct TypedBodyPlan {
    pub guards: Vec<BodyGuard>,
    /// Facts the typed body may rely on (param LocalId → type).
    pub proofs: HashMap<u32, Type>,
    /// Sites whose lowering the proofs change (the admission count).
    pub sites: usize,
    /// Class parameters (param LocalId → the class-id set its entry check
    /// proves), answered as receiver facts inside the typed body.
    pub receivers: std::collections::BTreeMap<u32, Vec<u32>>,
}

impl TypedBodyPlan {
    /// The module's elision facts plus this body's class parameters.
    pub(crate) fn receiver_facts(
        &self,
        base: &std::sync::Arc<super::ElisionFacts>,
    ) -> std::sync::Arc<super::ElisionFacts> {
        if self.receivers.is_empty() {
            return base.clone();
        }
        let mut f = (**base).clone();
        for (id, cids) in &self.receivers {
            f.locals.insert(
                *id,
                super::FactsRecv {
                    class_ids: cids.clone(),
                    nullable: false,
                },
            );
        }
        std::sync::Arc::new(f)
    }
}

pub(crate) fn typed_body_name(public: &str) -> String {
    format!("{public}$tbody")
}

pub(crate) fn generic_body_name(public: &str) -> String {
    format!("{public}$generic")
}

/// The class-id set of a class type (`C`, or a union of up to four
/// classes), when every member is a class with an id whose layout facts the
/// module has (prefix-stable, `wpfacts`).
fn class_set(
    ty: &Type,
    class_ids: &HashMap<String, u32>,
    facts: &super::ElisionFacts,
) -> Option<Vec<u32>> {
    let one = |t: &Type| match t {
        Type::Named(c) => class_ids
            .get(c)
            .copied()
            .filter(|id| *id != 0 && facts.classes.contains_key(id)),
        _ => None,
    };
    let mut ids = match ty {
        Type::Named(_) => vec![one(ty)?],
        Type::Union(v) if (1..=4).contains(&v.len()) => {
            v.iter().map(one).collect::<Option<Vec<u32>>>()?
        }
        _ => return None,
    };
    ids.sort_unstable();
    ids.dedup();
    Some(ids)
}

/// Field sites on the parameter `id` (closure bodies excluded) whose key is
/// a definite birth field of every class in `cids`: the sites the receiver
/// proof turns into case 1 or case 2.
fn receiver_sites(body: &[Stmt], id: u32, cids: &[u32], facts: &super::ElisionFacts) -> usize {
    let mut n = 0usize;
    let mut visit = |e: &Expr| {
        let (object, property) = match e {
            Expr::PropertyGet {
                object, property, ..
            }
            | Expr::PropertySet {
                object, property, ..
            }
            | Expr::PropertyUpdate {
                object, property, ..
            } => (object, property),
            _ => return,
        };
        if matches!(object.as_ref(), Expr::LocalGet(l) if *l == id)
            && cids.iter().all(|c| facts.field(*c, property).is_some())
        {
            n += 1;
        }
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

/// Plan the typed body of the closure `(params, body)`, or `None`: only a
/// closure with a class parameter that has at least one field site the
/// receiver proof elides (step 5b) gets one.
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
    facts: &super::ElisionFacts,
) -> Option<TypedBodyPlan> {
    if facts.classes.is_empty() {
        return None;
    }
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
    let mut guards = Vec::with_capacity(params.len());
    let mut receivers = std::collections::BTreeMap::new();
    let mut sites = 0usize;
    for p in params {
        // The binding must hold the entry value for the whole body.
        let stable = !boxed_vars.contains(&p.id) && !reassigned.contains(&p.id);
        // A declared class type, else fninfer's inferred class set.
        let inferred;
        let ty = match (&p.ty, facts.receiver_params.get(&p.id)) {
            (Type::Any | Type::Unknown, Some(names)) => {
                inferred = match names.as_slice() {
                    [one] => Type::Named(one.clone()),
                    many => Type::Union(many.iter().cloned().map(Type::Named).collect()),
                };
                &inferred
            }
            (ty, _) => ty,
        };
        let set = class_set(ty, class_ids, facts).filter(|_| stable);
        let n = set
            .as_ref()
            .map_or(0, |cids| receiver_sites(body, p.id, cids, facts));
        match set {
            Some(class_ids) if n > 0 => {
                sites += n;
                receivers.insert(p.id, class_ids.clone());
                guards.push(BodyGuard::Class { class_ids });
            }
            _ => guards.push(BodyGuard::None),
        }
    }
    if receivers.is_empty() {
        return None;
    }
    Some(TypedBodyPlan {
        guards,
        proofs: HashMap::new(),
        sites,
        receivers,
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
                BodyGuard::Class { class_ids } => {
                    let bits = blk.bitcast_double_to_i64(arg);
                    let recv = crate::expr::receiver_range::emit_fused_receiver_test(blk, &bits);
                    let handle = crate::expr::receiver_range::emit_handle(blk, &recv.biased);
                    let zero = blk.ptrtoint(&null_guard, I64);
                    let addr = blk.select(I1, &recv.is_object_pointer, I64, &handle, &zero);
                    let ptr = blk.inttoptr(I64, &addr);
                    let cid = blk.load(I32, &ptr);
                    let mut same = blk.icmp_eq(I32, &cid, &class_ids[0].to_string());
                    for c in &class_ids[1..] {
                        let eq = blk.icmp_eq(I32, &cid, &c.to_string());
                        same = blk.or(I1, &same, &eq);
                    }
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
    facts: &super::ElisionFacts,
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
        facts,
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
    let mut elided = |i: usize| llmod.function_mut(i).map_or(0, |f| f.wpfacts_sites);
    let (gen_sites, tb_sites) = (elided(generic_index), elided(tbody_index));
    // Step 5b: the clone must carry receiver-proven sites the generic body
    // does not have, and call fewer runtime entries.
    let installed = tb_sites > gen_sites && tb_calls < gen_calls;
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
    use crate::codegen::{ElisionFacts, FactsClass, FactsField};
    use perry_hir::types::Type;
    use perry_hir::{Expr, Param, Stmt};

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

    fn facts() -> ElisionFacts {
        let mut f = ElisionFacts::default();
        for (cid, slot) in [(7u32, 0u32), (8, 2)] {
            f.classes.insert(
                cid,
                FactsClass {
                    display: format!("C{cid}"),
                    fixed_layout: true,
                    fields: vec![FactsField {
                        key: "x".into(),
                        slot,
                        readonly: false,
                        kind_mask: 1,
                        ta_stable: false,
                        ta_kind: None,
                    }],
                },
            );
        }
        f
    }

    fn ids() -> HashMap<String, u32> {
        HashMap::from([("C".to_string(), 7u32), ("D".to_string(), 8u32)])
    }

    fn read_x(id: u32) -> Stmt {
        Stmt::Return(Some(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(id)),
            property: "x".into(),
            byte_offset: 0,
        }))
    }

    fn call() -> Stmt {
        Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::LocalGet(9)),
            args: Vec::new(),
            type_args: Vec::new(),
            byte_offset: 0,
        })
    }

    fn plan(params: &[Param], body: &[Stmt], f: &ElisionFacts) -> Option<TypedBodyPlan> {
        closure_typed_body_plan(
            params,
            body,
            false,
            false,
            false,
            &HashSet::new(),
            &HashSet::new(),
            &ids(),
            f,
        )
    }

    #[test]
    fn a_class_receiver_with_a_field_site_is_planned_and_survives_a_call() {
        let p = plan(
            &[param(1, Type::Named("C".into()))],
            &[call(), read_x(1)],
            &facts(),
        )
        .expect("plan");
        assert_eq!(p.guards, vec![BodyGuard::Class { class_ids: vec![7] }]);
        assert_eq!(p.receivers.get(&1), Some(&vec![7]));
        assert_eq!(p.sites, 1);
        assert!(p.proofs.is_empty());
    }

    #[test]
    fn an_inferred_receiver_of_an_untyped_param_is_planned() {
        let mut f = facts();
        f.receiver_params.insert(1, vec!["C".into(), "D".into()]);
        let p = plan(&[param(1, Type::Any)], &[read_x(1)], &f).expect("plan");
        assert_eq!(p.receivers.get(&1), Some(&vec![7, 8]));
    }

    #[test]
    fn a_class_union_is_one_set_guard() {
        let ty = Type::Union(vec![Type::Named("D".into()), Type::Named("C".into())]);
        let p = plan(&[param(1, ty)], &[read_x(1)], &facts()).expect("plan");
        assert_eq!(
            p.guards,
            vec![BodyGuard::Class {
                class_ids: vec![7, 8]
            }]
        );
    }

    #[test]
    fn no_facts_scalars_unknown_keys_or_reassigned_params_give_no_plan() {
        let c = || param(1, Type::Named("C".into()));
        assert!(plan(&[c()], &[read_x(1)], &ElisionFacts::default()).is_none());
        assert!(plan(&[param(1, Type::Number)], &[read_x(1)], &facts()).is_none());
        let other = Stmt::Return(Some(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(1)),
            property: "y".into(),
            byte_offset: 0,
        }));
        assert!(plan(&[c()], &[other], &facts()).is_none());
        let reassigned = HashSet::from([1u32]);
        assert!(closure_typed_body_plan(
            &[c()],
            &[read_x(1)],
            false,
            false,
            false,
            &HashSet::new(),
            &reassigned,
            &ids(),
            &facts(),
        )
        .is_none());
    }
}
