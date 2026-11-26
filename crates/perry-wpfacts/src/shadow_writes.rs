//! GUARDS3: what may shadow a class method (the prototype-census shadow
//! tests' proof). Per class id, the keys a program write may give an
//! instance of the class or its prototype, whether some write may give
//! either one any key, and whether the prototype's [[Prototype]] may change.
//! Codegen (`proto_constants`) drops a binding site's shadow-bit tests for a
//! chain whose name none of these reaches: the receiver gains no own key of
//! that name, no prototype before the holder gains it or is relinked, and the
//! holder keeps it. Everything here is the solver's own record of the writes
//! it saw (named and computed-key writers, accessor definitions, HARD marks
//! that may write any key, ⊤ stores into escaped objects, relinks); a class
//! the solver does not own (another unit's, an unsettled parent) gets no
//! entry, which is no proof.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::{Key, Model, TmplKind, TokKind, TOP};
use crate::pts::FxHashMap;

/// The blockers of one class id (`ProgramFacts::shadow_writes`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ShadowWrites {
    /// Keys some write may give an instance (an object whose class id this is).
    pub inst_names: BTreeSet<String>,
    /// Why an instance may gain any key or a new [[Prototype]].
    pub inst_any: Option<String>,
    /// Keys some write may give, change or delete on the class prototype
    /// after the class definition.
    pub proto_names: BTreeSet<String>,
    /// Why the prototype may gain, change or lose any key.
    pub proto_any: Option<String>,
    /// Why the prototype's [[Prototype]] may change.
    pub proto_relinked: Option<String>,
}

impl ShadowWrites {
    fn any(slot: &mut Option<String>, why: impl FnOnce() -> String) {
        if slot.is_none() {
            *slot = Some(why());
        }
    }
}

pub(crate) fn shadow_writes(m: &Model) -> BTreeMap<u32, ShadowWrites> {
    let mut out: BTreeMap<u32, ShadowWrites> = BTreeMap::new();
    if m.unit_mode {
        return out;
    }
    // The names a computed key may stand for that matter here: the keys of
    // the class prototypes.
    let mut proto_keys: BTreeSet<crate::model::Atom> = BTreeSet::new();
    for &(t, a) in m.fields.keys() {
        if matches!(m.toks[t as usize], TokKind::Proto(_)) {
            proto_keys.insert(a);
        }
    }
    let mut insts: FxHashMap<u32, Vec<u32>> = FxHashMap::default();
    for (t, kind) in m.toks.iter().enumerate() {
        if let TokKind::Obj(tm, _) = kind {
            insts.entry(*tm).or_default().push(t as u32);
        }
    }
    let numeric_key = |site: u32| {
        m.push_fill_sites.contains_key(&site)
            || m.stores[site as usize].key_value.is_some_and(|n| {
                let n = &m.nodes[n as usize];
                n.prim == crate::model::prim::NUM
                    && n.pts
                        .to_sorted()
                        .into_iter()
                        .all(|v| v != TOP && m.beside_values(v))
            })
    };
    let atom = |a: crate::model::Atom| m.atoms[a as usize].clone();
    // Keys a computed-key store under `key` may name (`None`: any key).
    let named_by = |key: Option<Key>| -> Option<Vec<String>> {
        match key {
            Some(k @ Key::Any(s)) if s != 0 => Some(
                proto_keys
                    .iter()
                    .filter(|&&a| m.key_may_name(k, a))
                    .map(|&a| atom(a))
                    .collect(),
            ),
            Some(Key::Named(a)) => Some(vec![atom(a)]),
            _ => None,
        }
    };
    // What a write into ⊤ may do to an escaped object.
    let top_names: Vec<String> = m.top_stored.iter().flatten().map(|&a| atom(a)).collect();
    let top_any = m.top_stored.contains(&None);
    let top_deleted: Vec<String> = m.top_reshaped_keys.iter().map(|&a| atom(a)).collect();
    let top_given = m.native_given.iter().any(|&(t, _)| t == TOP);
    // One object's blockers (`names`, `any`); `relinked` its [[Prototype]].
    let object = |t: u32,
                  escaped_class: bool,
                  names: &mut BTreeSet<String>,
                  any: &mut Option<String>,
                  relinked: &mut Option<String>| {
        if let Some(keys) = m.accessor_keys.get(&t) {
            names.extend(keys.iter().map(|&a| atom(a)));
        }
        if m.hard.contains_key(&t) && !m.hard_unseeded.contains(&t) {
            let (k, r) = m.hard[&t];
            ShadowWrites::any(any, || format!("hard {} ({})", k.code(), m.reasons[r as usize]));
        }
        if m.is_escaped_writer(t) {
            ShadowWrites::any(any, || {
                format!("escaped writer ({})", m.writer_escape_why(t).unwrap_or_default())
            });
        }
        if m.super_relink.contains(&t) || m.json_relink.contains(&t) {
            ShadowWrites::any(relinked, || "relink".to_string());
        }
        if escaped_class || m.is_escaped(t) {
            names.extend(top_names.iter().cloned());
            names.extend(top_deleted.iter().cloned());
            if top_any {
                ShadowWrites::any(any, || "top store under a computed key".to_string());
            }
            if m.top_reshaped_any {
                ShadowWrites::any(any, || "top delete under a computed key".to_string());
            }
            if top_given {
                ShadowWrites::any(relinked, || "setPrototypeOf on top".to_string());
            }
        }
    };
    for (tm, tpl) in m.tmpls.iter().enumerate() {
        let tm = tm as u32;
        let TmplKind::Class { class_id, .. } = tpl.kind else {
            continue;
        };
        if m.foreign.contains_key(&tm) || m.parent_unsettled.contains(&tm) {
            out.remove(&class_id);
            continue;
        }
        let w = out.entry(class_id).or_default();
        let escaped_class = m.virt_escaped.contains(&tm);
        // Instances.
        for (&(owner, a), fw) in m.writers.iter() {
            if owner == tm && (!fw.ctor.is_empty() || !fw.post.is_empty()) {
                w.inst_names.insert(atom(a));
            }
        }
        for (&(owner, a), fw) in m.proto_writers.iter() {
            if owner != tm {
                continue;
            }
            // The class definition's own (birth / rule 3) stores define the
            // prototype's members; any other store writes one.
            let later = fw.ctor.iter().chain(&fw.post).any(|&s| {
                let st = &m.stores[s as usize];
                !st.birth && st.proto_first != Some(tm)
            });
            if later {
                w.proto_names.insert(atom(a));
            }
        }
        if let Some(aw) = m.any_writers.get(&tm) {
            // Computed-key stores of the template reach its instances or its
            // prototype: both count them.
            let sites = aw
                .ctor
                .iter()
                .map(|&s| (s, None))
                .chain(aw.post.iter().map(|&(s, k)| (s, Some(k))));
            for (s, k) in sites {
                if numeric_key(s) {
                    continue;
                }
                // The store writes only the objects its receiver set held:
                // an instance, the class object (on no instance's chain), or
                // the prototype.
                let objs = m.any_writer_objs.get(&(tm, s)).copied().unwrap_or(7);
                let (inst, proto) = (objs & 1 != 0, objs & 4 != 0);
                match named_by(k) {
                    Some(ns) => {
                        if inst {
                            w.inst_names.extend(ns.iter().cloned());
                        }
                        if proto {
                            w.proto_names.extend(ns);
                        }
                    }
                    None => {
                        let st = &m.stores[s as usize];
                        let why = || {
                            let f = m.funcs.get(st.func as usize);
                            format!(
                                "computed-key store {} in {}",
                                st.desc,
                                f.map(|f| f.name.as_str()).unwrap_or("?")
                            )
                        };
                        if inst {
                            ShadowWrites::any(&mut w.inst_any, why);
                        }
                        if proto {
                            ShadowWrites::any(&mut w.proto_any, why);
                        }
                    }
                }
            }
        }
        let (mut names, mut any, mut relinked) = (BTreeSet::new(), None, None);
        for &t in insts.get(&tm).map(|v| v.as_slice()).unwrap_or(&[]) {
            object(t, escaped_class, &mut names, &mut any, &mut relinked);
        }
        w.inst_names.extend(names);
        if let Some(r) = relinked {
            ShadowWrites::any(&mut w.inst_any, || format!("instance {r}"));
        }
        if let Some(a) = any {
            ShadowWrites::any(&mut w.inst_any, || a);
        }
        if let Some(&p) = m.tok_index.get(&TokKind::Proto(tm)) {
            let (mut names, mut any, mut relinked) = (BTreeSet::new(), None, None);
            object(p, escaped_class, &mut names, &mut any, &mut relinked);
            w.proto_names.extend(names);
            if let Some(a) = any {
                ShadowWrites::any(&mut w.proto_any, || a);
            }
            if let Some(r) = relinked {
                ShadowWrites::any(&mut w.proto_relinked, || r);
            }
        }
    }
    // A type-specialized layout's class (`constructor_origins`) shares its
    // source class's prototype and writes: each sees the other's blockers.
    for (&clone, &origin) in m.constructor_origins.iter() {
        let cid = |tm: u32| match m.tmpls[tm as usize].kind {
            TmplKind::Class { class_id, .. } => Some(class_id),
            _ => None,
        };
        let (Some(a), Some(b)) = (cid(clone), cid(origin)) else {
            continue;
        };
        if a == b {
            continue;
        }
        let (Some(wa), Some(wb)) = (out.get(&a).cloned(), out.get(&b).cloned()) else {
            out.remove(&a);
            out.remove(&b);
            continue;
        };
        for (id, other) in [(a, wb), (b, wa)] {
            let w = out.get_mut(&id).expect("present");
            w.inst_names.extend(other.inst_names);
            w.proto_names.extend(other.proto_names);
            for (slot, why) in [
                (&mut w.inst_any, other.inst_any),
                (&mut w.proto_any, other.proto_any),
                (&mut w.proto_relinked, other.proto_relinked),
            ] {
                if slot.is_none() {
                    *slot = why;
                }
            }
        }
    }
    // A class id one of whose templates was dropped above is no proof.
    for (tm, tpl) in m.tmpls.iter().enumerate() {
        if let TmplKind::Class { class_id, .. } = tpl.kind {
            if m.foreign.contains_key(&(tm as u32)) || m.parent_unsettled.contains(&(tm as u32)) {
                out.remove(&class_id);
            }
        }
    }
    out
}
