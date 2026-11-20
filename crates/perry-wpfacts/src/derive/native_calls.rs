//! Shared native-member publication used by lowering and dispatch certificates.
use std::collections::BTreeSet;

/// MATHB1: the verdict of a call of a builtin namespace member `N.m`
/// ([`super::namespace_callee`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IntrinsicCallee {
    /// The callee is exactly the realm's original `N.m` (named `N.m`).
    Exact(String),
    /// Refused, with the reason.
    No(String),
}

/// The driver's native-member offsets. NativeMember sites seed the set;
/// namespace pairs add exact builtin callees only if no twin is unproven.
/// An explicitly refused namespace twin removes the offset in either case.
/// Dispatch certificates additionally require every named and HIR twin to
/// satisfy their stricter route/operand proof.
pub fn native_member_calls_of(
    pf: &crate::ProgramFacts,
    module: usize,
    namespace_pairs: bool,
) -> BTreeSet<u32> {
    let mut out: BTreeSet<u32> = pf
        .sites
        .iter()
        .filter(|s| {
            s.module as usize == module
                && s.is_call
                && matches!(s.verdict, crate::Verdict::NativeMember)
        })
        .map(|s| s.byte_offset)
        .collect();
    if !namespace_pairs {
        return out;
    }
    let here = |s: &&crate::SiteFacts| s.module as usize == module && s.is_call;
    let exact = |s: &crate::SiteFacts| {
        matches!(&s.intrinsic_callee, Some(IntrinsicCallee::Exact(n))
            if n.rsplit('.').next() == Some(s.key.as_str()))
    };
    let mut unproven = BTreeSet::new();
    let mut refused = BTreeSet::new();
    for s in pf.sites.iter().filter(here) {
        if !exact(s) && !matches!(s.verdict, crate::Verdict::NativeMember) {
            unproven.insert(s.byte_offset);
            if s.intrinsic_callee.is_some() {
                refused.insert(s.byte_offset);
            }
        }
    }
    for s in pf.sites.iter().filter(here) {
        if exact(s) && !unproven.contains(&s.byte_offset) {
            out.insert(s.byte_offset);
        }
    }
    out.retain(|o| !refused.contains(o));
    out
}
