//! The calls a dispatch on the receiver's class alone may answer.

use super::{lookup_holders, TemplateFacts};
use crate::gen::Gen;
use crate::model::*;
use crate::pts::FxHashMap;

/// Does every receiver of a call of `key` resolve it to the one method its class
/// or an ancestor declares under that name (`SiteFacts::closed_methods`)?
pub(super) fn closed_methods(
    g: &Gen<'_>,
    by_id: &FxHashMap<TmplId, &TemplateFacts>,
    toks: &[Tok],
    key: &str,
    a: Atom,
) -> bool {
    let m = &g.m;
    let declared = |holder: Tok, f: FnIdx| {
        let (TokKind::Proto(tm), fi) = (&m.toks[holder as usize], &m.funcs[f as usize]) else {
            return false;
        };
        let TmplKind::Class { module, name, .. } = &m.tmpls[*tm as usize].kind else {
            return false;
        };
        fi.module == *module
            && g.modules[*module as usize]
                .1
                .classes
                .iter()
                .find(|c| c.name == *name)
                .and_then(|c| c.methods.iter().rev().find(|x| x.name == key))
                .is_some_and(|x| x.id == fi.func_id)
    };
    let mut objects = toks
        .iter()
        .copied()
        .filter(|&t| !m.closed_marker(t))
        .peekable();
    objects.peek().is_some()
        && objects.all(|t| {
            let TokKind::Obj(tm, _) = &m.toks[t as usize] else {
                return false;
            };
            let Some(tf) = by_id.get(tm) else {
                return false;
            };
            let Ok(holders) = lookup_holders(m, tf, t, key, a) else {
                return false;
            };
            let [(holder, n)] = holders[..] else {
                return false;
            };
            let node = &m.nodes[n as usize];
            match node.pts.to_sorted()[..] {
                [v] if node.prim == 0 => {
                    matches!(m.toks[v as usize], TokKind::Func(f, _) if declared(holder, f))
                }
                _ => false,
            }
        })
}
