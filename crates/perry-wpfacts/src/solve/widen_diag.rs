//! Widening diagnostics (WID, reports 2026-10-18-wid-design.md): what a set
//! holds when it widens, and a what-if that keeps sets exact while their
//! tokens fall into few groups. Both are off by default and change nothing
//! when off; the what-if is the ceiling of any group-level widening and is
//! never used for real builds.

use crate::model::*;
use crate::pts::FxHashSet;

/// A token's group under the most permissive candidate partition: every
/// callable one group, arrays one, literal and anon-shape objects by birth
/// keys, builtin objects by kind, everything else itself.
fn group(m: &Model, t: Tok) -> (u8, u64) {
    match &m.toks[t as usize] {
        TokKind::Func(_) | TokKind::Bound(_) | TokKind::ClassObj(_) | TokKind::BuiltinFn => (1, 0),
        TokKind::Obj(tm) => {
            let tp = &m.tmpls[*tm as usize];
            match &tp.kind {
                TmplKind::Arr { .. } => (2, 0),
                TmplKind::BObj { what, .. } => (3, hash_str(what)),
                TmplKind::Lit { .. }
                | TmplKind::Class {
                    literal_shape: true,
                    ..
                } => (4, hash_keys(&tp.birth_keys)),
                _ => (5, t as u64),
            }
        }
        _ => (6, t as u64),
    }
}

fn hash_str(s: &str) -> u64 {
    s.bytes().fold(7u64, |a, b| {
        a.wrapping_mul(1_000_003).wrapping_add(b as u64)
    })
}

fn hash_keys(k: &[Atom]) -> u64 {
    k.iter().fold(7u64, |a, &b| {
        a.wrapping_mul(1_000_003).wrapping_add(b as u64)
    })
}

impl Model {
    /// The `widen_groups` what-if: true keeps node `n` exact past the bound.
    pub(super) fn widen_groups_keep(&mut self, n: NodeId) -> bool {
        if self.widen_groups == 0 {
            return false;
        }
        let mut v = Vec::new();
        self.nodes[n as usize].pts.sorted_into(&mut v);
        let groups: FxHashSet<(u8, u64)> = v.iter().map(|&t| group(self, t)).collect();
        let keep = groups.len() <= self.widen_groups;
        if keep && self.nodes[n as usize].pts.len() == self.widen_k + 1 {
            self.widen_kept += 1;
        }
        keep
    }

    /// One `perry: widen-census` line for node `n`, about to widen: token
    /// kinds, whether ⊤ is already in it (a cascade) or not (a root), and
    /// its group counts: f0 tokens (= families), f1 objects by layout with
    /// callables per body, f2 f1 with callables per module, f3 f1 with all
    /// callables one. Roots also name their first members.
    pub(super) fn widen_census_line(&self, n: NodeId) {
        let toks = self.nodes[n as usize].pts.to_sorted();
        let mut kinds: std::collections::BTreeMap<&str, u32> = Default::default();
        let mut fam: [FxHashSet<(u8, u64)>; 3] = Default::default();
        let mut has_top = false;
        for &t in &toks {
            let (k, by_module) = match &self.toks[t as usize] {
                TokKind::Top => ("top", None),
                TokKind::Obj(tm) => (
                    match &self.tmpls[*tm as usize].kind {
                        TmplKind::Class {
                            literal_shape: true,
                            ..
                        } => "anonshape",
                        TmplKind::Class { .. } => "class",
                        TmplKind::Lit { .. } => "lit",
                        TmplKind::FnInst { .. } => "fninst",
                        TmplKind::ObjCreate { .. } => "objcreate",
                        TmplKind::Arr { .. } => "arr",
                        TmplKind::BObj { .. } => "bobj",
                    },
                    None,
                ),
                TokKind::Func(f) => ("func", Some(self.funcs[*f as usize].module as u64)),
                TokKind::Bound(_) => ("bound", Some(u64::MAX)),
                TokKind::ClassObj(_) => ("classobj", None),
                TokKind::Proto(_) => ("proto", None),
                TokKind::ModNs(_) => ("modns", None),
                TokKind::ObjectProto => ("objproto", None),
                TokKind::Builtin(_) => ("builtin", None),
                TokKind::BuiltinFn => ("builtinfn", None),
                TokKind::PrimBox => ("primbox", None),
                TokKind::NativeProto(_) => ("native", None),
            };
            has_top |= t == TOP;
            *kinds.entry(k).or_default() += 1;
            let g = group(self, t);
            let callable = g.0 == 1;
            fam[0].insert(if callable { (7, t as u64) } else { g });
            fam[1].insert(match by_module {
                Some(md) if callable => (8, md),
                _ if callable => (7, t as u64),
                _ => g,
            });
            fam[2].insert(g);
        }
        let ks: Vec<String> = kinds.iter().map(|(k, v)| format!("{k}:{v}")).collect();
        let label = self.labels[n as usize];
        eprintln!(
            "perry: widen-census seq={} label={label} size={} top={} f0={} f1={} f2={} f3={} kinds={}",
            self.widened_nodes,
            toks.len(),
            has_top as u8,
            toks.len(),
            fam[0].len(),
            fam[1].len(),
            fam[2].len(),
            ks.join(",")
        );
        if !has_top {
            let names: Vec<String> = toks
                .iter()
                .filter_map(|&t| match &self.toks[t as usize] {
                    TokKind::Func(f) => Some(self.funcs[*f as usize].name.clone()),
                    TokKind::Obj(tm) => Some(self.tmpls[*tm as usize].display.clone()),
                    _ => None,
                })
                .take(8)
                .collect();
            eprintln!(
                "perry: widen-root seq={} label={label} names={}",
                self.widened_nodes,
                names.join(" | ")
            );
        }
    }
}
