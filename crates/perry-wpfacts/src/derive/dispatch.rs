//! Certificates for named calls whose emitted route cannot enter the tower.
use crate::{derive::Access, gen::Gen, ProgramFacts, SiteFacts, Verdict};
use perry_hir::{walker, Class, Expr, Function};

#[derive(Clone, Copy, Debug)]
pub(crate) enum Certificate {
    /// `lower_call` -> `try_lower_proven_method_call`, with an ordinary
    /// same-class holder and a nonnullable `this`; no guard or fallback.
    DirectThisMethod,
}

fn ordinary(f: &Function) -> bool {
    !f.is_async
        && !f.is_generator
        && !f.was_plain_async
        && f.decorators.is_empty()
        && !perry_hir::fn_proto::function_body_returns_generator_object(&f.body)
        && !f.body.iter().any(|st| {
            matches!(st, perry_hir::Stmt::Return(Some(e))
            if perry_hir::fn_proto::is_record_generator_birth(e))
        })
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
                && *byte_offset == s.byte_offset && *property == s.key)
        {
            return true;
        }
    }
    let mut found = false;
    walker::walk_expr_children(e, &mut |c| found |= names_call(c, s));
    found
}

fn direct_this(g: &Gen<'_>, s: &SiteFacts) -> bool {
    if !s.is_call
        || s.access != Access::Read
        || s.top
        || s.receiver_prims != 0
        || s.receiver_values.is_empty()
        || s.byte_offset == 0
        || s.key.starts_with('#')
        || !matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set)
    {
        return false;
    }
    let (Some(cid), [(mi, fid)]) = (s.holder_class, s.target_fns.as_slice()) else {
        return false;
    };
    if *mi != s.module {
        return false;
    }
    let Some((_, hir)) = g.modules.get(s.module as usize) else {
        return false;
    };
    let Some(c) = hir.classes.iter().find(|c| c.id == cid && root_class(c)) else {
        return false;
    };
    if hir.classes.iter().filter(|other| other.id == cid).count() != 1
        || hir
            .classes
            .iter()
            .filter(|other| other.name == c.name)
            .count()
            != 1
    {
        return false;
    }
    let Some(target) = c.methods.iter().rev().find(|f| f.name == s.key) else {
        return false;
    };
    if target.id != *fid || !ordinary(target) {
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

pub(crate) fn certify(pf: &mut ProgramFacts, g: &Gen<'_>, unit: bool) {
    // The driver refuses all method-call elisions in these worlds.
    if unit
        || pf.stats.aborted
        || pf.interp_guard.is_some()
        || pf.sabotage.is_some()
        || matches!(
            std::env::var("PERRY_FACTS_ELIDE").as_deref(),
            Ok("0" | "off" | "false")
        )
    {
        return;
    }
    for s in &mut pf.sites {
        if direct_this(g, s) {
            s.dispatch_certificate = Some(Certificate::DirectThisMethod);
        }
    }
    // program_facts::elision_for requires every record at an offset/key
    // to agree. A refused twin keeps the generic route for all of them.
    let refused: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.is_call && s.dispatch_certificate.is_none())
        .map(|s| (s.module, s.byte_offset, s.key.clone()))
        .collect();
    for s in &mut pf.sites {
        if refused.contains(&(s.module, s.byte_offset, s.key.clone())) {
            s.dispatch_certificate = None;
        }
    }
}
