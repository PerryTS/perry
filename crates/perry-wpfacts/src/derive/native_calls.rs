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
/// A refusal always wins: a twin whose namespace verdict refuses the
/// callee (`IntrinsicCallee::No`, or an exact callee of another name)
/// removes the offset, even one a NativeMember verdict seeded and with
/// namespace pairs off (CWN ruling 2026-11-24, STOP10).
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
    let here = |s: &&crate::SiteFacts| s.module as usize == module && s.is_call;
    let exact = |s: &crate::SiteFacts| {
        matches!(&s.intrinsic_callee, Some(IntrinsicCallee::Exact(n))
            if n.rsplit('.').next() == Some(s.key.as_str()))
    };
    let refused: BTreeSet<u32> = pf
        .sites
        .iter()
        .filter(here)
        .filter(|s| s.intrinsic_callee.is_some() && !exact(s))
        .map(|s| s.byte_offset)
        .collect();
    if namespace_pairs {
        let unproven: BTreeSet<u32> = pf
            .sites
            .iter()
            .filter(here)
            .filter(|s| !exact(s) && !matches!(s.verdict, crate::Verdict::NativeMember))
            .map(|s| s.byte_offset)
            .collect();
        for s in pf.sites.iter().filter(here) {
            if exact(s) && !unproven.contains(&s.byte_offset) {
                out.insert(s.byte_offset);
            }
        }
    }
    out.retain(|o| !refused.contains(o));
    out
}
