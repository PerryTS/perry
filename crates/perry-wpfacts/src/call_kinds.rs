//! The primitive kinds of a call's result (`ModuleBindings::call_kinds`).
//!
//! A call is a **primitive call** when, at every copy of the call the
//! program holds, its result node holds no object and no ⊤: only the
//! primitives its `prim` mask names. The exported mask is the union over the
//! copies. Codegen reads two shapes of it (`expr::facts_string`): `STR`
//! alone (the result is a string, so `f(a) + g(b)` is a concatenation) and
//! a subset of `STR | NULL | UNDEF` with `STR` (`f(a) ?? ""` is a string).
//!
//! Keyed by the call's own span (`Expr::Call::byte_offset`, never 0, and
//! `byte_end`): `a.b().c()` holds two calls that start at `a`, and the span
//! tells them apart. Calls that share a span (copies of one source call: a
//! body walked twice, a callee the inliner copied) are joined: one copy that
//! is no primitive call withdraws the site.
//!
//! The fact selects a lowering whose every arm is still chosen by the value
//! at run time (`js_string_concat_box` dispatches on the tags), so a wrong
//! claim cannot change an answer; check mode witnesses it all the same
//! (`Fact::CallKinds`). Negative control `PERRY_FACTS_SABOTAGE=stringcall`:
//! every call with an offset is claimed `STR`.
//!
//! `PERRY_FACTS_CALL_KINDS=0` (also `off`, `false`) exports nothing.

use crate::bindings::ModuleBindings;
use crate::gen::Gen;
use crate::model::prim;

/// `PERRY_FACTS_CALL_KINDS=0` exports no call kinds.
pub fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_CALL_KINDS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// Fill `ModuleBindings::call_kinds` (module docs). `probe`: one
/// `# call_kinds module= offset= end= kinds=` line per exported site.
pub(crate) fn collect(
    g: &Gen<'_>,
    bindings: &mut [ModuleBindings],
    sabotage: Option<&str>,
    mut probe: Option<&mut Vec<String>>,
) {
    if g.m.aborted {
        return;
    }
    let claim_all = sabotage == Some("stringcall");
    // (module, offset, end) -> the union of the copies' kinds, or None once
    // a copy holds an object or ⊤.
    let mut sites: std::collections::BTreeMap<(u32, u32, u32), Option<u16>> = Default::default();
    for &(mi, offset, end, n) in &g.call_kinds_at {
        let node = &g.m.nodes[n as usize];
        let kinds = if claim_all {
            Some(prim::STR)
        } else if node.pts.is_empty() {
            Some(node.prim)
        } else {
            None
        };
        let entry = sites.entry((mi, offset, end)).or_insert(Some(0));
        *entry = match (*entry, kinds) {
            (Some(prev), Some(k)) => Some(prev | k),
            _ => None,
        };
    }
    for ((mi, offset, end), kinds) in sites {
        let (Some(kinds), Some(mb)) = (kinds, bindings.get_mut(mi as usize)) else {
            continue;
        };
        // A result that holds nothing is a call that never returns here:
        // nothing to say about it.
        if kinds == 0 {
            continue;
        }
        mb.call_kinds.push((offset, end, kinds));
        if let Some(p) = probe.as_deref_mut() {
            p.push(format!(
                "# call_kinds module={mi} offset={offset} end={end} kinds={}",
                prim::name(kinds)
            ));
        }
    }
}
