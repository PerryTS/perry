//! Shared method-call elision and exact dispatcher-route certificates.
use crate::{derive::Access, gen::Gen, DispatchObligation, ProgramFacts, SiteFacts, Verdict};
use perry_hir::{walker, Class, Expr, Function};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Certificate {
    /// `lower_call` -> `try_lower_proven_method_call`, with an ordinary
    /// same-class holder and a nonnullable `this`; no guard or fallback.
    DirectThisMethod,
}

/// A solve-backed method-call elision. Only `dispatch` additionally proves
/// that the exact lowered route cannot redispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MethodCallCertificate {
    pub module: u32,
    pub class: u32,
    pub function: u32,
    pub nullable: bool,
    pub(crate) dispatch: Option<Certificate>,
}

/// The A/B control shared by fact certification and every driver elision.
pub fn elide_enabled() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_ELIDE").as_deref(),
        Ok("0" | "off" | "false")
    )
}

impl SiteFacts {
    pub fn method_call_certificate(&self) -> Option<&MethodCallCertificate> {
        elide_enabled()
            .then_some(self.method_call.as_ref())
            .flatten()
    }
}

fn ordinary(f: &Function) -> bool {
    perry_hir::method_call::ordinary_entry(f)
        && !f.was_plain_async
        && f.decorators.is_empty()
        && !perry_hir::fn_proto::function_body_returns_generator_object(&f.body)
}

fn root_class(c: &Class) -> bool {
    c.extends.is_none()
        && c.extends_name.is_none()
        && c.extends_expr.is_none()
        && c.native_extends.is_none()
        && c.computed_members.is_empty()
        && c.decorators.is_empty()
        && c.static_accessor_names.is_empty()
}

fn names_call(e: &Expr, s: &SiteFacts) -> bool {
    // A nested closure has a different entry and this binding.
    if matches!(e, Expr::Closure { .. }) {
        return false;
    }
    if let Expr::Call { callee, .. } = e {
        if matches!(callee.as_ref(), Expr::PropertyGet { object, property, byte_offset, .. }
            if matches!(object.as_ref(), Expr::This)
                && *byte_offset == s.byte_offset && *property == s.key
                && perry_hir::method_call::named_site(property, *byte_offset))
        {
            return true;
        }
    }
    let mut found = false;
    walker::walk_expr_children(e, &mut |c| found |= names_call(c, s));
    found
}

fn direct_this(s: &SiteFacts, c: &Class, target: &Function, holder_module: u32) -> bool {
    if s.access != Access::Read
        || s.receiver_prims != 0
        || s.receiver_values.is_empty()
        || !perry_hir::method_call::named_site(&s.key, s.byte_offset)
    {
        return false;
    }
    if holder_module != s.module {
        return false;
    }
    if !root_class(c) || !ordinary(target) {
        return false;
    }
    // The exact call form must hold in every generating body. Neither a
    // T0 value call nor a spread/borrowed call uses this codegen route.
    !s.bodies.is_empty()
        && s.bodies.iter().all(|(id, _)| {
            c.methods.iter().chain(c.constructor.iter()).any(|f| {
                f.id == *id
                    && ordinary(f)
                    && f.body
                        .iter()
                        .any(|st| walker::stmt_any_expr(st, &mut |e| names_call(e, s)))
            })
        })
}

/// Resolve every twin once, using borrowed keys and copyable certificates.
/// A refusal is sticky; nullable receivers are joined, and the narrower
/// dispatch proof survives only when every twin carries it.
pub(crate) fn agree(sites: &mut [SiteFacts]) {
    let mut groups = HashMap::new();
    for s in sites.iter().filter(|s| s.is_call) {
        let at = (s.module, s.byte_offset, s.key.as_str());
        let current = groups.entry(at).or_insert(s.method_call);
        match (current.as_mut(), s.method_call) {
            (Some(a), Some(b))
                if (a.module, a.class, a.function) == (b.module, b.class, b.function) =>
            {
                a.nullable |= b.nullable;
                if b.dispatch.is_none() {
                    a.dispatch = None;
                }
            }
            _ => *current = None,
        }
    }
    let settled: Vec<_> = sites
        .iter()
        .map(|s| {
            groups
                .get(&(s.module, s.byte_offset, s.key.as_str()))
                .copied()
                .flatten()
        })
        .collect();
    drop(groups);
    for (s, certificate) in sites.iter_mut().zip(settled) {
        s.method_call = s.is_call.then_some(certificate).flatten();
    }
}

pub(crate) fn certify(pf: &mut ProgramFacts, g: &Gen<'_>, cfg: &crate::FactsConfig) {
    if cfg.unit
        || pf.stats.aborted
        || !elide_enabled()
        || (pf.interp_guard.is_some() && pf.sabotage.as_deref() != Some("unchecked"))
    {
        return;
    }
    // Witness routes and sabotage can still use lookup elisions, but cannot
    // certify the audited, unguarded route into the exact method body.
    let direct = !cfg.verify_facts && pf.interp_guard.is_none() && pf.sabotage.is_none();
    for s in &mut pf.sites {
        if !s.is_call
            || s.top
            || s.receiver_prims & !(crate::model::prim::NULL | crate::model::prim::UNDEF) != 0
            || !matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set)
        {
            continue;
        }
        let (Some(class), [(module, function)]) = (s.holder_class, s.target_fns.as_slice()) else {
            continue;
        };
        let Some((_, hir)) = g.modules.get(*module as usize) else {
            continue;
        };
        let Some((c, target)) = perry_hir::method_call::target(hir, class, *function, &s.key)
        else {
            continue;
        };
        s.method_call = Some(MethodCallCertificate {
            module: *module,
            class,
            function: *function,
            nullable: s.receiver_prims != 0,
            dispatch: (direct && direct_this(s, c, target, *module))
                .then_some(Certificate::DirectThisMethod),
        });
    }
    agree(&mut pf.sites);
}

impl SiteFacts {
    /// The site's obligation: `Some` for a member call (a read or a store
    /// calls no method by itself; its accessor's call is the accessor's
    /// site).
    pub fn dispatch_obligation(&self) -> Option<DispatchObligation<'_>> {
        (self.is_call && self.access == Access::Read).then_some(DispatchObligation {
            key: &self.key,
            receivers: &self.receiver_values,
            prims: self.receiver_prims,
            op: None,
            namespace: None,
            redispatch: self
                .method_call_certificate()
                .and_then(|c| c.dispatch)
                .is_none(),
        })
    }
}
