//! PR-C2 (DESIGN-p4l1 §4.2): the integer interval of an element read's key,
//! for the rep-keyed element tier.
//!
//! An element read `a[k]` is in the floor of its receiver's `Rep::Arr` when
//! `k` is an integer in `[0, floor)`. The receiver's rep is the binding's
//! (`BindingFacts::rep`); this is the other half, the key at the site: the
//! hull of the key's interval over every copy of the read the program holds
//! (inlining and an enclosing for-of rewrite duplicate a read; a summarized
//! body has one copy per instance), in the form `call_arg_ranges` has for a
//! call's arguments. A copy whose key may be anything but an integer Number
//! drops the site. Reads that are not copies of one another may share an
//! offset (in `a[i][0]` both member expressions start at `a`): the hull
//! then covers both keys, which says less.
//!
//! Exported with `Model::array_reps` on only (`PERRY_FACTS_ARRAY_REPS=0`
//! exports nothing).
//!
//! **Key signs** (`collect_signs`, `ModuleBindings::key_signs`). An element
//! site of any operation (a read, a store, a called element) whose key is an
//! integer Number inside the int32 range at every copy, and at every site
//! that shares its offset, with the hull of its interval. Only the key
//! node's own interval counts: a key the solve resolves to the name `"3"`
//! may be that string at run time. Such a key is its `i32` exactly: the site needs no key normalization (a fraction, NaN, a
//! string or a symbol never reaches it). With `lo >= 0` it is an array
//! index, so the site needs no sign test and no property-key arm either.
//! The hull is over reads, stores and calls alike, so a store that shares
//! its offset with a read (`a[i] += 1`) has one verdict.
//! `PERRY_FACTS_KEY_SIGN=0` (`off`, `false`) exports none. The negative
//! control `PERRY_FACTS_SABOTAGE=keysign` claims every int32 key
//! non-negative; the check-mode witness at the site (`perry-codegen`
//! `expr::index_key::witness_sign`) must catch it.

use crate::bindings::{number_range, ModuleBindings};
use crate::gen::Gen;
use crate::model::{Key, NodeId};

/// The key interval of one copy: a canonical index literal, or the key
/// node's integer interval.
fn copy_key(g: &Gen<'_>, key: Key, idx: NodeId) -> Option<(i64, i64)> {
    match key {
        Key::Named(a) => {
            let k = &g.m.atoms[a as usize];
            let v = k.parse::<u32>().ok().filter(|v| v.to_string() == *k)?;
            Some((i64::from(v), i64::from(v)))
        }
        Key::Any(_) => number_range(&g.m, idx),
    }
}

/// `PERRY_FACTS_KEY_SIGN=0` turns the key signs off.
pub(crate) fn sign_enabled() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_KEY_SIGN").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// `(module, offset)` -> the hull of the key over every copy of every site
/// there that `admit` takes, or `None` once a copy has no interval.
type Hulls = std::collections::BTreeMap<(u32, u32), Option<(i64, i64)>>;

/// Fill `ModuleBindings::element_keys` (module docs).
pub(crate) fn collect(g: &Gen<'_>, bindings: &mut [ModuleBindings]) {
    if g.m.aborted || !g.m.array_reps {
        return;
    }
    let read = |op: crate::derive::ElementOp| op == crate::derive::ElementOp::Read;
    for ((mi, offset), k) in hulls(g, read, true) {
        if let (Some(k), Some(mb)) = (k, bindings.get_mut(mi as usize)) {
            mb.element_keys.push((offset, k));
        }
    }
}

/// The key `k` is inside the int32 range.
fn int32(k: (i64, i64)) -> bool {
    k.0 >= i64::from(i32::MIN) && k.1 <= i64::from(i32::MAX)
}

/// Fill `ModuleBindings::key_signs` (module docs).
pub(crate) fn collect_signs(g: &Gen<'_>, bindings: &mut [ModuleBindings]) {
    if g.m.aborted || !g.m.key_sign {
        return;
    }
    for ((mi, offset), k) in hulls(g, |_| true, false) {
        let Some(k) = k.filter(|&k| int32(k)) else {
            continue;
        };
        let k = if g.m.key_sign_sabotage {
            (0, k.1.max(0))
        } else {
            k
        };
        if let Some(mb) = bindings.get_mut(mi as usize) {
            mb.key_signs.push((offset, k));
        }
    }
}

/// The probe (`PERRY_FACTS_DUMP`): one `# key_sign` line per element site
/// whose key is an integer Number at every copy, with its interval and the
/// verdicts (`int32`: no normalization; `nonneg`: no sign test).
pub(crate) fn probe(g: &Gen<'_>) -> Vec<String> {
    if !g.m.key_sign {
        return Vec::new();
    }
    let mut ops: std::collections::BTreeMap<(u32, u32), (bool, bool)> = Default::default();
    for site in &g.element_sites {
        let e = ops.entry((site.module, site.byte_offset)).or_default();
        match site.op {
            crate::derive::ElementOp::Write => e.1 = true,
            _ => e.0 = true,
        }
    }
    hulls(g, |_| true, false)
        .into_iter()
        .filter_map(|((mi, offset), k)| {
            let (lo, hi) = k?;
            let (r, w) = ops.get(&(mi, offset)).copied().unwrap_or_default();
            Some(format!(
                "# key_sign module={mi} site={offset} op={}{} key=[{lo},{hi}] int32={} nonneg={}",
                if r { "r" } else { "" },
                if w { "w" } else { "" },
                int32((lo, hi)) as u8,
                (int32((lo, hi)) && lo >= 0) as u8
            ))
        })
        .collect()
}

/// The key hulls of the sites whose operation `admit` takes. `named`: a
/// canonical index literal counts as its index (`copy_key`); otherwise only
/// the key node's own interval does, so a key that is the string `"3"` at
/// run time has none.
fn hulls(g: &Gen<'_>, admit: impl Fn(crate::derive::ElementOp) -> bool, named: bool) -> Hulls {
    // A summarized body's key node has one copy per instance (`Inst::fresh`).
    let mut copies: std::collections::HashMap<NodeId, Vec<NodeId>> = Default::default();
    for inst in 0..g.m.sum.insts.len() as u32 {
        g.m.inst_copies(inst, |orig, copy| {
            copies.entry(orig).or_default().push(copy)
        });
    }
    // (module, offset) -> Some(hull), or None once a copy has no interval.
    let mut sites: Hulls = Default::default();
    let hull = |a: Option<(i64, i64)>, b: Option<(i64, i64)>| match (a, b) {
        (Some((a, b)), Some((c, d))) => Some((a.min(c), b.max(d))),
        _ => None,
    };
    for site in &g.element_sites {
        if !admit(site.op) || site.byte_offset == 0 {
            continue;
        }
        let mut k = if named {
            copy_key(g, site.key, site.idx)
        } else {
            number_range(&g.m, site.idx)
        };
        if !named || matches!(site.key, Key::Any(_)) {
            for &c in copies.get(&site.idx).into_iter().flatten() {
                k = hull(k, number_range(&g.m, c));
            }
        }
        sites
            .entry((site.module, site.byte_offset))
            .and_modify(|e| *e = hull(*e, k))
            .or_insert(k);
    }
    sites
}
