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

/// The method call offsets of module `module` proven to read their intrinsic
/// member: the NativeMember verdicts, and (MATHC1, the namespace pairs on)
/// the namespace calls whose callee is exactly the original builtin `N.m`.
/// The set is keyed by offset alone, so such an offset joins only when every
/// call site there is proven (NativeMember or exact), and one holding an
/// unproven namespace call leaves it (a chain `Math.f(x).g()` shares the
/// offset).
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
