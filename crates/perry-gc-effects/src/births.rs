//! Keeps the declared births of the byte-cell family complete
//! (DESIGN-family-reach section 11, condition 3).
//!
//! The whole-program fact `family_reach` says "this program never creates a
//! typed array, Buffer or ArrayBuffer" from three things: what the program
//! text names, what the realm declares born by the built-ins the program
//! observes (`births` on a realm declaration), and unknown values. The
//! second is a list people write. This check derives the same statement
//! from the runtime's machine code and fails when the list is short.
//!
//! Every byte cell that is a JavaScript value is allocated by one function
//! (`buffer::store::store_alloc`: the `birth` rule of
//! `scripts/gc_call_effects/byte_cell_births.txt`). From the call graph the
//! GC classes come from, a realm native's body either cannot reach it, or:
//!
//! - the native is an entry of the family, or of a family that requires it
//!   (`perry_runtime_manifest::families`): observing it makes the family
//!   live;
//! - it declares the birth: an entry of the family (or of one that
//!   requires it) is in its own `births`. Its holder's `births` do not
//!   count: the fact observes members one by one;
//! - otherwise the check FAILS and prints the call path. A native whose
//!   body symbol is not in the analyzed archives (a member that shares the
//!   bound-method sentinel, a body in another archive) is counted as
//!   unchecked unless one of the two holds for it by name.
//!
//! The walk stops at an `in-hand` function: one that runs only when a byte
//! cell already exists (its receiver or an argument is one), or that
//! dispatches on a value the program holds (a member of the family is then
//! reached only with the family live, and a member of core is a row of this
//! check itself). Each cut is a line of the rules file with its reason, and
//! a rule that matches no function fails the check (a renamed function
//! would otherwise empty a cut, or the seed).
//!
//! A birth can also start outside every native's body: in a function the
//! runtime reaches only through a pointer (a completion the event loop
//! runs, a callback handed to the host, a scheduled task). Such a function
//! has no caller in the graph, or its address is taken in code. Each one
//! that can reach the birth must match an `origin` rule, whose reason says
//! which observation of the program schedules it; one that matches none
//! FAILS. (An entry point compiled code calls is listed in the census
//! only: what compiled code may call is the fact's node classification,
//! `family_reach/nodes.rs`. An address held only in data, a vtable or a
//! static table, is not seen.)
//!
//! A claim is set by the image's installer at the end of `js_gc_init`
//! (perry-runtime `family_never`). A `before-claim` rule names what runs
//! before that; such a function that can reach the birth FAILS.
//!
//! The graph is an upper bound (a body that could reach the allocation only
//! with a byte cell in hand still reaches it here unless a cut says so), so
//! over-reporting forces a declaration or a stated cut, never a wrong fact.
//! Values handed to callbacks and events, or stored in result fields, need
//! no rule of their own: whatever the native does with the cell, its body
//! allocated it.
use crate::effects::Native;
use crate::graph::Graph;
use crate::rules::Rule;
use anyhow::{bail, Context, Result};
use perry_runtime_manifest::families::{self, FamilySet};
use std::collections::{BTreeMap, HashMap, VecDeque};

/// The rules of the check, compiled into the analyzer.
pub const RULES: &str = include_str!("../../../scripts/gc_call_effects/byte_cell_births.txt");

/// The family the check is about.
pub const FAMILY: &str = "typedarray";

/// The `births` of every realm declaration, by name.
pub fn declared_births(manifest: &str) -> Result<HashMap<String, Vec<String>>> {
    let v: serde_json::Value = serde_json::from_str(manifest).context("realm manifest JSON")?;
    let decls = v["declarations"]
        .as_array()
        .context("manifest has no declarations")?;
    let mut out = HashMap::new();
    for d in decls {
        let Some(name) = d["name"].as_str() else {
            continue;
        };
        let births = d
            .get("births")
            .and_then(|b| b.as_array())
            .map(|b| {
                b.iter()
                    .filter_map(|n| n.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        out.insert(name.to_owned(), births);
    }
    Ok(out)
}

/// The families observing entry `name` makes live: its own and what they
/// require. A name the table does not classify is every family.
fn observed(name: &str) -> FamilySet {
    families::close_requires(families::entry_families(name).unwrap_or_else(families::all_families))
}

/// Why native `name` may bear a byte cell without the fact missing it;
/// `None`: nothing says so.
pub fn covered(name: &str, births: &HashMap<String, Vec<String>>) -> Option<&'static str> {
    let bit = families::family_bit(FAMILY);
    if observed(name) & bit != 0 {
        return Some("family");
    }
    let bears = |holder: &str| {
        births
            .get(holder)
            .is_some_and(|b| b.iter().any(|b| observed(b) & bit != 0))
    };
    if bears(name) {
        return Some("declared");
    }
    None
}

#[derive(Default)]
pub struct Report {
    pub lines: Vec<String>,
    pub census: String,
    /// The functions that can reach a birth and that nothing in the graph
    /// calls, or whose address code takes (`label<TAB>kind`): realm native
    /// bodies, entry points compiled code calls, and functions reached only
    /// through a pointer (a callback, a completion the event loop runs).
    pub roots: String,
    pub failures: usize,
}

fn path(g: &Graph, next: &[Option<usize>], mut i: usize) -> String {
    let mut steps = vec![g.label(i)];
    while let Some(n) = next[i].filter(|&n| n != i) {
        i = n;
        steps.push(g.label(i));
        if steps.len() > 40 {
            steps.push("...".into());
            break;
        }
    }
    steps
        .iter()
        .map(|s| s.chars().take(120).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n      -> ")
}

/// Checks every realm native against the graph. `indirect_pred` are the
/// delegated indirect edges of the GC rules (a hook wrapper's callees).
pub fn check(
    g: &Graph,
    indirect_pred: &[Vec<usize>],
    rules: &[Rule],
    natives: &[Native],
    births: &HashMap<String, Vec<String>>,
    undeclared: &[String],
) -> Result<Report> {
    let nn = g.nodes.len();
    let mut seed = vec![false; nn];
    let mut cut = vec![false; nn];
    let mut origin = vec![false; nn];
    let mut early = vec![false; nn];
    let mut hits = vec![0usize; rules.len()];
    for i in 0..nn {
        let spellings = g.spellings(i);
        for (r, rule) in rules.iter().enumerate() {
            if spellings.iter().any(|s| rule.matches(s)) {
                hits[r] += 1;
                match rule.kind.as_str() {
                    "birth" => seed[i] = true,
                    "in-hand" => cut[i] = true,
                    "origin" => origin[i] = true,
                    "before-claim" => early[i] = true,
                    other => bail!("{}: kind {other} is no byte-cell rule", rule.description()),
                }
            }
        }
    }
    let dead: Vec<String> = rules
        .iter()
        .zip(&hits)
        .filter(|(_, &n)| n == 0)
        .map(|(r, _)| r.description().replace("seeds.txt", "byte_cell_births.txt"))
        .collect();
    if !dead.is_empty() {
        bail!(
            "byte-cell rules that match no function (the check would be vacuous):\n  {}",
            dead.join("\n  ")
        );
    }
    if !seed.iter().any(|&s| s) {
        bail!("no birth rule: the check would be vacuous");
    }
    // Backward from the birth; a cut function is not entered (a seed that
    // is also cut stays a seed for its own callers' cuts to decide).
    let mut next: Vec<Option<usize>> = vec![None; nn];
    let mut work = VecDeque::new();
    for i in 0..nn {
        if seed[i] {
            next[i] = Some(i);
            work.push_back(i);
        }
    }
    while let Some(t) = work.pop_front() {
        for &c in g.pred[t].iter().chain(&indirect_pred[t]) {
            if next[c].is_none() && !cut[c] {
                next[c] = Some(t);
                work.push_back(c);
            }
        }
    }
    let mut report = Report::default();
    // What runs before an image's claim is set must bear no byte cell: the
    // claim would not see it.
    for i in (0..nn).filter(|&i| early[i] && next[i].is_some()) {
        report.failures += 1;
        report.lines.push(format!(
            "{}: FAIL: it runs before the image's family claim is set and can reach a \
             byte-cell birth\n      {}",
            g.label(i),
            path(g, &next, i)
        ));
    }
    let mut by_name: HashMap<&str, Vec<usize>> = HashMap::new();
    for i in 0..nn {
        for s in g.spellings(i) {
            by_name.entry(s).or_default().push(i);
        }
    }
    let native_symbols: std::collections::HashSet<&str> =
        natives.iter().map(|n| n.symbol.as_str()).collect();
    let mut roots = Vec::new();
    let mut origins = (0usize, 0usize);
    for i in 0..nn {
        if next[i].is_none() {
            continue;
        }
        let called = g.pred[i]
            .iter()
            .chain(&indirect_pred[i])
            .any(|&c| c != i && next[c].is_some());
        // A delegated callee (a hook's operation) is reached through its
        // wrapper, which the walk already follows.
        let by_pointer = g.taken[i] && indirect_pred[i].is_empty();
        if called && !by_pointer {
            continue;
        }
        let spellings = g.spellings(i);
        let kind =
            if spellings.iter().any(|s| native_symbols.contains(s)) {
                "native"
            } else if !by_pointer
                && spellings
                    .iter()
                    .any(|s| !s.contains("::") && !s.starts_with('_'))
            {
                "exported"
            } else if origin[i] {
                origins.0 += 1;
                "by pointer, origin stated"
            } else {
                origins.1 += 1;
                report.failures += 1;
                report.lines.push(format!(
                "{}: FAIL: it can reach a byte-cell birth and is reached only through a pointer \
                 ({}), and no origin rule says what schedules it\n      {}",
                g.label(i),
                if called { "its address is taken" } else { "no caller" },
                path(g, &next, i)
            ));
                "by pointer, NO ORIGIN"
            };
        roots.push(format!("{}\t{kind}", g.label(i)));
    }
    roots.sort();
    report.roots = roots.join("\n") + "\n";
    report
        .census
        .push_str("name\tsymbol\tfound\treaches\tverdict\n");
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut unchecked = 0usize;
    for n in natives {
        let nodes = by_name.get(n.symbol.as_str());
        let reach = nodes.and_then(|ns| ns.iter().copied().find(|&i| next[i].is_some()));
        let known = Some(&n.name)
            .filter(|name| !undeclared.contains(name))
            .and_then(|name| covered(name, births));
        let verdict = match (nodes, reach) {
            (None, _) if known.is_some() => format!("not in the graph ({})", known.unwrap()),
            (None, _) => {
                unchecked += 1;
                "not in the graph, UNCHECKED".to_owned()
            }
            (Some(_), None) => "no birth".to_owned(),
            (Some(_), Some(i)) => match known {
                Some(why) => why.to_owned(),
                None => {
                    report.failures += 1;
                    report.lines.push(format!(
                        "{}: FAIL: its body can reach a byte-cell birth, and neither its family \
                         nor a declared birth says so\n      {}",
                        n.name,
                        path(g, &next, i)
                    ));
                    "FAIL".to_owned()
                }
            },
        };
        *counts.entry(verdict.clone()).or_default() += 1;
        report.census.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\n",
            n.name,
            n.symbol,
            nodes.is_some() as u8,
            reach.is_some() as u8,
            verdict
        ));
    }
    report.lines.push(format!(
        "births-census: natives={} {}",
        natives.len(),
        counts
            .iter()
            .map(|(k, v)| format!("{}={v}", k.replace(' ', "_")))
            .collect::<Vec<_>>()
            .join(" ")
    ));
    report.lines.push(format!(
        "births-check: family={FAMILY} rules={} seeds={} cuts={} reaching={} origins={} stated={} unstated={} unchecked_natives={unchecked} failures={}",
        rules.len(),
        seed.iter().filter(|&&s| s).count(),
        cut.iter().filter(|&&c| c).count(),
        next.iter().filter(|n| n.is_some()).count(),
        roots.len(),
        origins.0,
        origins.1,
        report.failures
    ));
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn births(pairs: &[(&str, &[&str])]) -> HashMap<String, Vec<String>> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.iter().map(|s| s.to_string()).collect()))
            .collect()
    }

    /// An entry of the family, or of a family that requires it, needs no
    /// declaration; a core native needs one; a core holder's does not count
    /// for its members.
    #[test]
    fn a_core_native_must_declare_the_birth() {
        let none = births(&[]);
        assert_eq!(covered("Uint8Array.from", &none), Some("family"));
        assert_eq!(covered("Buffer.alloc", &none), Some("family"));
        assert_eq!(covered("String.prototype.split", &none), None);
        let declared = births(&[("String.prototype.split", &["Uint8Array"])]);
        assert_eq!(
            covered("String.prototype.split", &declared),
            Some("declared")
        );
        // A birth of another family does not cover it.
        let other = births(&[("String.prototype.split", &["RegExp"])]);
        assert_eq!(covered("String.prototype.split", &other), None);
        // The holder's declaration does not cover its members.
        let holder = births(&[("String", &["Uint8Array"])]);
        assert_eq!(covered("String.prototype.split", &holder), None);
    }

    /// The rules file parses and holds one birth.
    #[test]
    fn the_rules_file_names_the_one_birth() {
        let rules = crate::rules::load_rules(RULES).unwrap();
        assert_eq!(rules.iter().filter(|r| r.kind == "birth").count(), 1);
        assert!(rules.iter().all(|r| matches!(
            r.kind.as_str(),
            "birth" | "in-hand" | "origin" | "before-claim"
        )));
    }

    #[test]
    fn the_manifest_births_are_read_by_name() {
        let manifest = r#"{"declarations":[{"name":"Array.from","births":["ArrayBuffer","Uint8Array"]},{"name":"atob"}]}"#;
        let b = declared_births(manifest).unwrap();
        assert_eq!(b["Array.from"], ["ArrayBuffer", "Uint8Array"]);
        assert!(b["atob"].is_empty());
        assert_eq!(covered("Array.from", &b), Some("declared"));
        assert_eq!(covered("atob", &b), None);
    }
}
