//! What keeps the driver going: a unit solve's output changes against its last solve, the no-fixpoint report, published contracts and captured diagnostics.

use super::*;

/// The rounds after which the driver names what keeps it going (the corpus
/// reaches its fixpoint in at most ~15). A round count that keeps growing is
/// a unit whose outputs never settle: each report at round 64, 128, 256, ...
/// names the units that round solved and what each changed.
pub(super) const LONG_ROUNDS: u32 = 64;

/// What a unit's solve changed in its outputs against its last solve: each
/// contract added (`+`), dropped (`-`) or changed, with the fields that
/// differ.
pub(super) fn output_changes(u: &ProgramFacts, old: &ProgramFacts) -> Vec<String> {
    fn keyed<K: std::fmt::Display + PartialEq, C: std::fmt::Debug + PartialEq>(
        out: &mut Vec<String>,
        what: &str,
        new: &[(K, &C)],
        old: &[(K, &C)],
    ) {
        for (k, c) in new {
            match old.iter().find(|(ok, _)| ok == k) {
                None => out.push(format!("+{what} {k}")),
                Some((_, p)) if p != c => {
                    out.push(format!("{what} {k} [{}]", changed_fields(*p, *c).join(" ")))
                }
                Some(_) => {}
            }
        }
        for (k, _) in old {
            if !new.iter().any(|(nk, _)| nk == k) {
                out.push(format!("-{what} {k}"));
            }
        }
    }
    fn own(v: &ProgramFacts) -> Vec<(String, &FnContract)> {
        v.own_fns_out.iter().map(|c| (c.name.clone(), c)).collect()
    }
    fn named<C>(v: &[((String, String), C)]) -> Vec<(String, &C)> {
        v.iter()
            .map(|(k, c)| (format!("{}:{}", k.0, k.1), c))
            .collect()
    }
    // A cell keeps the unit's last write joined with its latest
    // (DECISIONS 197): what it holds after this solve is that join.
    let joined: Vec<(String, ValueContract)> = old
        .cells_out
        .iter()
        .map(|(k, c)| {
            let mut j = c.clone();
            if let Some((_, n)) = u.cells_out.iter().find(|(q, _)| q == k) {
                j.join(n);
            }
            (k.clone(), j)
        })
        .chain(
            u.cells_out
                .iter()
                .filter(|(k, _)| !old.cells_out.iter().any(|(q, _)| q == k))
                .cloned(),
        )
        .collect();
    fn cells(v: &[(String, ValueContract)]) -> Vec<(String, &ValueContract)> {
        v.iter().map(|(k, c)| (k.clone(), c)).collect()
    }
    let mut out = Vec::new();
    keyed(&mut out, "own_fn", &own(u), &own(old));
    keyed(
        &mut out,
        "fn",
        &named(&u.fn_contracts_out),
        &named(&old.fn_contracts_out),
    );
    keyed(
        &mut out,
        "class",
        &named(&u.contracts_out),
        &named(&old.contracts_out),
    );
    keyed(
        &mut out,
        "value",
        &named(&u.values_out),
        &named(&old.values_out),
    );
    keyed(&mut out, "cell", &cells(&joined), &cells(&old.cells_out));
    // Every other output the driver publishes or feeds back to the units:
    // a change in any of them re-seeds a reader, so each is compared (a
    // forwarded layout replaced each round once read as "nothing").
    fn by_display(v: &[ClassContract]) -> Vec<(String, &ClassContract)> {
        v.iter().map(|c| (c.display.clone(), c)).collect()
    }
    fn plain<C>(v: &[(String, C)]) -> Vec<(String, &C)> {
        v.iter().map(|(k, c)| (k.clone(), c)).collect()
    }
    fn stores(v: &[(String, String, ValueContract)]) -> Vec<(String, &ValueContract)> {
        v.iter().map(|(d, k, c)| (format!("{d}.{k}"), c)).collect()
    }
    keyed(
        &mut out,
        "layout",
        &by_display(&u.layouts_out),
        &by_display(&old.layouts_out),
    );
    keyed(&mut out, "args", &plain(&u.args_out), &plain(&old.args_out));
    keyed(
        &mut out,
        "ctor_args",
        &named(&u.ctor_args_out),
        &named(&old.ctor_args_out),
    );
    keyed(
        &mut out,
        "store",
        &stores(&u.stores_out),
        &stores(&old.stores_out),
    );
    keyed(
        &mut out,
        "write",
        &named(&u.writes_out),
        &named(&old.writes_out),
    );
    keyed(
        &mut out,
        "this_args",
        &plain(&u.this_args_out),
        &plain(&old.this_args_out),
    );
    keyed(
        &mut out,
        "method_args",
        &plain(&u.method_args_out),
        &plain(&old.method_args_out),
    );
    keyed(
        &mut out,
        "ctor_runs",
        &plain(&u.ctor_runs_out),
        &plain(&old.ctor_runs_out),
    );
    for (what, new, was) in [
        ("fn defined", &u.fns_defined, &old.fns_defined),
        ("method run", &u.method_runs_out, &old.method_runs_out),
        ("forwarded layout", &u.layouts_fwd, &old.layouts_fwd),
        ("report", &u.reports, &old.reports),
    ] {
        for n in new.iter().filter(|n| !was.contains(n)) {
            out.push(format!("+{what} {n}"));
        }
        for n in was.iter().filter(|n| !new.contains(n)) {
            out.push(format!("-{what} {n}"));
        }
    }
    let added = u
        .shared_out
        .iter()
        .filter(|w| !old.shared_out.contains(w))
        .count();
    let gone = old
        .shared_out
        .iter()
        .filter(|w| !u.shared_out.contains(w))
        .count();
    if added + gone != 0 {
        out.push(format!("shared writes +{added} -{gone}"));
    }
    out
}

/// The top-level fields of `a` and `b` (one type) whose values differ, read
/// from their derived `Debug` form: no list of fields to keep in step.
pub(crate) fn changed_fields<T: std::fmt::Debug>(a: &T, b: &T) -> Vec<String> {
    fn fields(s: &str) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = Vec::new();
        for line in s.lines() {
            let top = line.starts_with("    ") && !line.starts_with("     ");
            match line[line.len().min(4)..].split_once(": ") {
                Some((name, rest)) if top => v.push((name.to_string(), rest.to_string())),
                _ => {
                    if let Some(last) = v.last_mut() {
                        last.1.push('\n');
                        last.1.push_str(line);
                    }
                }
            }
        }
        v
    }
    let (fa, fb) = (fields(&format!("{a:#?}")), fields(&format!("{b:#?}")));
    let mut out: Vec<String> = fa
        .iter()
        .filter(|(n, x)| fb.iter().find(|(m, _)| m == n).is_none_or(|(_, y)| x != y))
        .map(|(n, _)| n.clone())
        .collect();
    out.extend(
        fb.iter()
            .filter(|(n, _)| !fa.iter().any(|(m, _)| m == n))
            .map(|(n, _)| n.clone()),
    );
    out
}

/// The no-fixpoint report of round `round`: the units it solved and what
/// each changed (at most 16 units, 8 changes each).
pub(super) fn no_fixpoint_report(
    round: u32,
    solves: &[Solve<'_>],
    changed: &[(usize, Vec<String>)],
    seeds: usize,
) {
    eprintln!(
        "perry: wpfacts: no fixpoint after {round} rounds: round {round} solved {} units, {seeds} seeded for the next",
        changed.len()
    );
    for (si, cs) in changed.iter().take(16) {
        let shown: Vec<&str> = cs.iter().take(8).map(|c| c.as_str()).collect();
        eprintln!(
            "perry: wpfacts:   {} changed {}: {}{}",
            solves[*si].key,
            cs.len(),
            if shown.is_empty() {
                "nothing".to_string()
            } else {
                shown.join("; ")
            },
            if cs.len() > 8 { "; ..." } else { "" }
        );
    }
}

/// A unit's class, function and value contracts as the next levels read them:
/// under the unit's path and, when the module exports the name, under the
/// module's.
pub(super) fn publish_contracts(input: &mut Published, s: &Solve, u: &ProgramFacts) {
    let module_export = |n: &String| !s.view.unit_exports.contains(n);
    for ((p, n), c) in &u.contracts_out {
        input.classes.insert((s.key.clone(), n.clone()), c.clone());
        if module_export(n) {
            input.classes.insert((p.clone(), n.clone()), c.clone());
        }
    }
    for ((p, n), c) in &u.fn_contracts_out {
        input.fns.insert((s.key.clone(), n.clone()), c.clone());
        if module_export(n) {
            input.fns.insert((p.clone(), n.clone()), c.clone());
        }
    }
    for ((p, n), c) in &u.values_out {
        input.values.insert((s.key.clone(), n.clone()), c.clone());
        if module_export(n) {
            input.values.insert((p.clone(), n.clone()), c.clone());
        }
    }
}

/// Dump only. Solves every unit once more at the fixpoint with the
/// diagnostics on, the way a round would (each level reads the contracts of
/// the earlier ones), and keeps from it what the dump needs: the sites and
/// the diagnostic records, written to the sidecar file when there is one
/// (their offsets are returned) instead of held. The facts are the rounds'.
pub(super) fn capture_diagnostics(
    per_unit: &mut [Option<ProgramFacts>],
    solves: &[Solve],
    sorted: &[(&str, &Module)],
    ucfg: &FactsConfig,
    input: &mut Published,
    levels: &[Vec<usize>],
    sidecar: Option<&std::path::Path>,
) -> Option<(std::path::PathBuf, Vec<(u64, u64)>)> {
    use std::io::Write;
    let dcfg = FactsConfig {
        diagnostics: true,
        ..ucfg.clone()
    };
    let mut out = sidecar.map(|p| {
        std::io::BufWriter::new(
            std::fs::File::create(p)
                .unwrap_or_else(|e| panic!("cannot write {}: {e}", p.display())),
        )
    });
    let mut spans = vec![(0u64, 0u64); solves.len()];
    let mut at = 0u64;
    input.classes.clear();
    input.fns.clear();
    input.values.clear();
    for level in levels {
        // Chunks bound what is held: a level can be thousands of units.
        for chunk in level.chunks(256) {
            for (si, mut u2) in solve_level(chunk, solves, sorted, &dcfg, input) {
                let u = per_unit[si].as_mut().expect("every unit solved");
                u.sites = std::mem::take(&mut u2.sites);
                u.diagnostic_sites = std::mem::take(&mut u2.diagnostic_sites);
                match out.as_mut() {
                    Some(w) => {
                        let mut n = 0u64;
                        for (_, lines) in &u2.diagnostics {
                            for line in lines {
                                let text = format!(
                                    "D {{\"unit\":{},{}\n",
                                    crate::diagnostics::quote(&solves[si].key),
                                    &line[1..]
                                );
                                w.write_all(text.as_bytes()).expect("write the sidecar");
                                n += text.len() as u64;
                            }
                        }
                        spans[si] = (at, n);
                        at += n;
                        u.diagnostics = u2
                            .diagnostics
                            .into_iter()
                            .map(|(k, _)| (k, Vec::new()))
                            .collect();
                    }
                    None => u.diagnostics = u2.diagnostics,
                }
            }
        }
        for &si in level {
            let u = per_unit[si].as_ref().expect("every unit solved");
            publish_contracts(input, &solves[si], u);
        }
    }
    match (out, sidecar) {
        (Some(mut w), Some(p)) => {
            w.flush().expect("write the sidecar");
            Some((p.to_path_buf(), spans))
        }
        _ => None,
    }
}
