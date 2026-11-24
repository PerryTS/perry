//! K3 census (report only, `PERRY_K3_CENSUS=<path>`): one `ESCF` line per
//! escaped function token, read from the solved shape: its escape reason,
//! whether that escape hands it to code that may write it (`writer`), the
//! keys it is held under (own data value or accessor half of a holder),
//! the number of tokens whose chain reaches such a holder (its family,
//! counted from the prototype edges), and the read sites of its body whose
//! receiver is its `this` or a parameter (all, and those whose receiver
//! holds ⊤). Nothing here changes a verdict.

use crate::gen::Gen;
use crate::model::{NodeId, Tok, TokKind};
use crate::pts::{FxHashMap, FxHashSet};
use std::fmt::Write as _;

pub(crate) fn write(g: &Gen) {
    let Some(path) = std::env::var_os("PERRY_K3_CENSUS") else {
        return;
    };
    let m = &g.m;
    let has_top = |n: NodeId| {
        m.nodes[n as usize]
            .pts
            .to_sorted()
            .iter()
            .any(|&x| matches!(m.toks[x as usize], TokKind::Top))
    };
    // Holders: (fn token) -> [(holder, key)] from the fields and accessors.
    let mut held: FxHashMap<Tok, Vec<(Tok, u32)>> = FxHashMap::default();
    for (&(h, a), &n) in &m.fields {
        for x in m.nodes[n as usize].pts.to_sorted() {
            if matches!(m.toks[x as usize], TokKind::Func(..)) {
                held.entry(x).or_default().push((h, a));
            }
        }
    }
    for (&(h, a), (gs, ss)) in &m.accessors {
        for &x in gs.iter().chain(ss) {
            held.entry(x).or_default().push((h, a));
        }
    }
    // Reverse prototype edges: p -> the tokens whose prototypes hold p.
    let mut rev: FxHashMap<Tok, Vec<Tok>> = FxHashMap::default();
    for (&t, &pn) in &m.proto_node {
        for p in m.nodes[pn as usize].pts.to_sorted() {
            rev.entry(p).or_default().push(t);
        }
    }
    // Receiver nodes of each body: its `this` and its parameters.
    let mut this_of: FxHashMap<NodeId, u32> = FxHashMap::default();
    let mut param_of: FxHashMap<NodeId, u32> = FxHashMap::default();
    for (f, fi) in m.funcs.iter().enumerate() {
        this_of.insert(fi.this, f as u32);
        for &p in &fi.params {
            param_of.insert(p, f as u32);
        }
    }
    for i in &m.sum.insts {
        this_of.insert(i.this, i.f);
        for &p in &i.params {
            param_of.insert(p, i.f);
        }
    }
    // (this all, this ⊤, param all, param ⊤) per function.
    let mut sites: FxHashMap<u32, [u32; 4]> = FxHashMap::default();
    for s in &g.read_sites {
        let top = has_top(s.recv);
        if let Some(&f) = this_of.get(&s.recv) {
            let e = sites.entry(f).or_default();
            e[0] += 1;
            e[1] += top as u32;
        } else if let Some(&f) = param_of.get(&s.recv) {
            let e = sites.entry(f).or_default();
            e[2] += 1;
            e[3] += top as u32;
        }
    }
    let mut out = String::new();
    let mut esc: Vec<(&Tok, &u32)> = m.escaped.iter().collect();
    esc.sort();
    for (&t, &r) in esc {
        let TokKind::Func(f, _) = m.toks[t as usize] else {
            continue;
        };
        let fi = &m.funcs[f as usize];
        let homes = held.get(&t).cloned().unwrap_or_default();
        let mut keys: Vec<&str> = homes.iter().map(|&(_, a)| m.atoms[a as usize].as_str()).collect();
        keys.sort();
        keys.dedup();
        let mut fam: FxHashSet<Tok> = FxHashSet::default();
        let mut work: Vec<Tok> = homes.iter().map(|&(h, _)| h).collect();
        while let Some(x) = work.pop() {
            if fam.insert(x) {
                work.extend(rev.get(&x).into_iter().flatten().copied());
            }
        }
        let s = sites.get(&f).copied().unwrap_or_default();
        let _ = writeln!(
            out,
            "ESCF\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            fi.name.replace('\t', " "),
            m.reasons[r as usize].replace('\t', " "),
            m.is_escaped_writer(t) as u8,
            fi.lexical_this as u8,
            if keys.is_empty() { "-".to_string() } else { keys.join(",") },
            fam.len(),
            s[0],
            s[1],
            s[2],
            s[3],
            m.writer_escape_why(t).unwrap_or_default().replace('\t', " "),
        );
    }
    let _ = std::fs::write(path, out);
}
