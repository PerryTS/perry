//! Merging a unit solve into the program: owners and dependents, the unit's bodies and site records, typed-parameter verdicts, binding collisions.

use super::*;

/// The units defining each display. A display may have several owners:
/// every unit holding a copy of one literal shape (`decl_units`).
pub(in crate::units) fn owners(per_unit: &[Option<ProgramFacts>]) -> FxHashMap<String, Vec<usize>> {
    let mut owner: FxHashMap<String, Vec<usize>> = FxHashMap::default();
    for (si, u) in per_unit.iter().enumerate() {
        let Some(u) = u else { continue };
        for t in u.templates.iter().filter(|t| t.module == Some(0)) {
            owner.entry(t.display.clone()).or_default().push(si);
        }
        for d in &u.fns_defined {
            owner.entry(d.clone()).or_default().push(si);
        }
    }
    owner
}

/// Units `work` and every unit depending on one of them, transitively;
/// a unit over the bound keeps its result (`resolve_again`).
pub(in crate::units) fn dependents(
    solves: &[Solve],
    per_unit: &[Option<ProgramFacts>],
    mut work: Vec<usize>,
) -> Vec<bool> {
    let n = solves.len();
    let mut importers: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (si, s) in solves.iter().enumerate() {
        for &d in &s.deps {
            importers[d].push(si);
        }
    }
    let mut dirty = vec![false; n];
    while let Some(si) = work.pop() {
        if per_unit[si].as_ref().is_some_and(|u| u.stats.aborted) {
            continue;
        }
        if !std::mem::replace(&mut dirty[si], true) {
            work.extend(importers[si].iter().copied());
        }
    }
    dirty
}

/// A unit's bodies by program module: its solve's module 0 is the unit's
/// own module (`mi`); a body of any other module is no claim here.
pub(in crate::units) fn program_bodies(bodies: &mut Vec<crate::bindings::BodyRef>, mi: usize) {
    if bodies.iter().any(|b| b.module != 0) {
        bodies.clear();
    }
    for b in bodies.iter_mut() {
        b.module = mi as u32;
    }
}

pub(in crate::units) fn merge_unit(
    pf: &mut ProgramFacts,
    mi: usize,
    path: &str,
    view: &crate::decl_units::UnitView,
    u: ProgramFacts,
) {
    for mut t in u.templates {
        if t.module != Some(0) {
            continue;
        }
        t.module = Some(mi as u32);
        for f in t.fields.iter_mut() {
            program_bodies(&mut f.fns, mi);
        }
        t.id = pf.templates.len() as crate::model::TmplId;
        pf.templates.push(t);
    }
    pf.diagnostic_sites.extend(u.diagnostic_sites);
    pf.diagnostics.extend(
        u.diagnostics
            .into_iter()
            .map(|(_, lines)| (path.to_owned(), lines)),
    );
    pf.sites.extend(u.sites.into_iter().map(|s| SiteFacts {
        diagnostic_unit: s.diagnostic_unit.as_ref().map(|_| path.to_owned()),
        module: mi as u32,
        ..s
    }));
    if let Some(mut b) = u.bindings.into_iter().next() {
        for (_, f) in b.locals.iter_mut().chain(b.this_of.iter_mut()) {
            program_bodies(&mut f.bodies, mi);
        }
        let into = &mut pf.bindings[mi];
        into.locals.extend(
            b.locals
                .into_iter()
                .filter(|(id, _)| view.binding_locals.contains(id)),
        );
        into.numeric_functions.extend(
            b.numeric_functions
                .into_iter()
                .filter(|(id, _, _)| view.binding_funcs.contains(id)),
        );
        into.this_of.extend(
            b.this_of
                .into_iter()
                .filter(|(id, _)| view.binding_funcs.contains(id)),
        );
    }
    pf.indexed_metadata_unwritten &= u.indexed_metadata_unwritten;
    pf.super_unrelinked.extend(u.super_unrelinked);
    pf.interp_guard = pf.interp_guard.take().or(u.interp_guard);
    pf.fnproto_bind_written = pf.fnproto_bind_written.take().or(u.fnproto_bind_written);
    pf.fnproto_borrow_written = pf
        .fnproto_borrow_written
        .take()
        .or(u.fnproto_borrow_written);
    pf.array_iteration_written = pf
        .array_iteration_written
        .take()
        .or(u.array_iteration_written);
    pf.array_iterator_close_written = pf
        .array_iterator_close_written
        .take()
        .or(u.array_iterator_close_written);
    let (a, s) = (&mut pf.stats, &u.stats);
    a.templates += s.templates;
    a.fact_units += s.fact_units;
    a.nodes += s.nodes;
    a.constraints += s.constraints;
    a.tokens += s.tokens;
    a.functions += s.functions;
    a.gen_ms += s.gen_ms;
    a.solve_ms += s.solve_ms;
    a.derive_ms += s.derive_ms;
    a.pops += s.pops;
    a.instances += s.instances;
    a.shared_instances += s.shared_instances;
    a.composed += s.composed;
    a.solver_calls += s.solver_calls;
    a.contexts += s.contexts;
    a.work += s.work;
    a.top_calls += s.top_calls;
    a.pts_entries += s.pts_entries;
    pf.units.push((path.to_string(), u.stats));
}

/// One fact per read site, as one run's (`derive::site_facts`): the records
/// of a site walked twice (an inlined body is a copy of its own) unite their
/// receivers. A unit's record no receiver or callee flows into (a dead
/// site: its body's own unit when the one caller runs an inlined copy)
/// adds nothing to that union: the other units' records decide the site.
/// A site only dead records name stays one dead fact.
pub(in crate::units) fn join_site_records(pf: &mut ProgramFacts) {
    let dead =
        |s: &SiteFacts| match &s.verdict {
            crate::Verdict::Dead => true,
            crate::Verdict::No(r) => r.starts_with("dead site"),
            _ => false,
        };
    let key = |s: &SiteFacts| (s.module, s.byte_offset, s.key.clone(), s.is_call);
    let live: FxHashSet<(u32, u32, String, bool)> =
        pf.sites.iter().filter(|s| !dead(s)).map(key).collect();
    let mut seen: FxHashSet<(u32, u32, String, bool)> = FxHashSet::default();
    pf.sites
        .retain(|s| !dead(s) || (!live.contains(&key(s)) && seen.insert(key(s))));
}

/// Post-census verdicts for sites on a parameter whose TS type is an
/// object type (design §11.4): the type picks the candidates, every class
/// template of the program whose definite data fields carry the type's
/// keys; the entry guard is `class_id(param) ∈ candidates`. The site is E0
/// when every candidate is prefix-stable and holds the key in one slot.
pub(in crate::units) fn typed_param_verdicts(pf: &mut ProgramFacts) {
    let mut cand_of: FxHashMap<Vec<String>, Vec<usize>> = FxHashMap::default();
    for s in pf.sites.iter_mut() {
        let Some(keys) = s.guard_keys.take() else {
            continue;
        };
        if !matches!(s.verdict, crate::Verdict::No(_)) || s.is_call {
            continue;
        }
        let tfs = &pf.templates;
        let cands = cand_of.entry(keys.clone()).or_insert_with(|| {
            (0..tfs.len())
                .filter(|&i| {
                    let t = &tfs[i];
                    t.class_id.is_some()
                        && keys.iter().all(|k| {
                            t.fields
                                .iter()
                                .any(|f| f.is(k, false) && f.definite && f.data_field)
                        })
                })
                .collect()
        });
        if cands.is_empty() {
            continue;
        }
        let slot = |t: &TemplateFacts| {
            t.fields
                .iter()
                .find(|f| f.key == s.key && f.definite && f.data_field)
                .and_then(|f| f.slot)
        };
        let first = slot(&tfs[cands[0]]);
        if first.is_some()
            && cands
                .iter()
                .all(|&i| tfs[i].prefix_stable && slot(&tfs[i]) == first)
        {
            s.receivers = cands.iter().map(|&i| tfs[i].display.clone()).collect();
            s.verdict = crate::Verdict::E0;
        }
    }
}

/// Facts must name one lexical binding. A second declaration with the same
/// id is a compiler bug, not a choice of which declaration's proof to keep.
pub(in crate::units) fn unique_bindings<T>(values: &mut [(u32, T)], module: &str, kind: &str) {
    values.sort_by_key(|(id, _)| *id);
    for pair in values.windows(2) {
        assert_ne!(
            pair[0].0, pair[1].0,
            "per-unit facts: duplicate {kind} {} in {module}; cloned bodies must have distinct binding identities",
            pair[0].0,
        );
    }
}
