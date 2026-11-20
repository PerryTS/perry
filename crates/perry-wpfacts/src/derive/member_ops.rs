//! Dispatcher obligations of HIR-specialized member operations.
use super::{element, exact_array, receiver_set, DispatchObligation};

impl crate::MemberOpFacts {
    /// The op's obligation (`member_ops`).
    pub fn dispatch_obligation(&self) -> DispatchObligation<'_> {
        DispatchObligation {
            key: &self.key,
            receivers: &self.receiver_values,
            prims: self.receiver_prims,
            op: Some(&self.op),
            namespace: self.namespace.as_deref(),
            redispatch: self.redispatch,
        }
    }
}

/// NDISP2: the published record of every HIR-specialized member op
/// (`member_ops`). A receiver's values are the unguarded solved set over
/// every run's copy of the node. The fallback edge of a folded Array op is
/// taken unless every value is an exact Array; of any other folded or
/// native receiver op, always, except native pushes whose published
/// method-site or fill certificate matches the codegen route.
pub(crate) fn member_op_facts(
    g: &crate::gen::Gen,
    bindings: &[crate::bindings::ModuleBindings],
    interpreted: bool,
) -> Vec<crate::MemberOpFacts> {
    let m = &g.m;
    let copies = element::run_copies(m);
    let elisions = !m.aborted
        && !m.unit_mode
        && !interpreted
        && !matches!(
            std::env::var("PERRY_FACTS_ELIDE").as_deref(),
            Ok("0" | "off" | "false")
        );
    g.member_ops
        .iter()
        .map(|o| {
            let key = m.atoms[o.key as usize].clone();
            let (receiver_values, receiver_prims, redispatch) = match o.recv {
                Some(r) => {
                    let all = element::with_copies(&copies, &[r]);
                    let set = receiver_set(m, &all);
                    let array_only = o.op.starts_with("Array")
                        && set.prims == 0
                        && !set.toks.is_empty()
                        && !all.iter().any(|&n| m.nodes[n as usize].dropped)
                        && set.toks.iter().all(|&t| exact_array(m, t));
                    // Native pushes use the exact certificates consumed by
                    // folded_builtin_override::native_push_call / try_lower:
                    // a rest fill has no guard; an Array-member site drops it.
                    let native_push = elisions
                        && o.native_push.is_some_and(|site| {
                            bindings.get(o.module as usize).is_some_and(|b| {
                                b.method_sites.iter().any(|(s, k, flags)| {
                                    *s == site
                                        && *k == key
                                        && flags & perry_hir::folded_call::SITE_ARRAY_MEMBER != 0
                                }) || matches!(site, perry_hir::folded_call::Site::Recv(offset)
                                if b.rest_pushes.iter().any(|(at, _)| *at == offset))
                            })
                        });
                    (
                        set.values(m),
                        set.prims,
                        !(native_push || (o.folded && array_only)),
                    )
                }
                None => (Vec::new(), 0, false),
            };
            crate::MemberOpFacts {
                module: o.module,
                op: o.op.clone(),
                key,
                namespace: o.namespace.clone(),
                receiver_values,
                receiver_prims,
                stated: o.recv.is_some() || o.namespace.is_some(),
                redispatch,
                form: o.form,
            }
        })
        .collect()
}
