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

/// The router every call of a bound-method member enters
/// (`closure::BOUND_METHOD_FUNC_PTR` is a sentinel, no code): it normalizes
/// the namespace's module name and calls the module's dispatch function
/// through the registry `js_nm_install_<module>` fills.
pub const ROUTER: &str =
    "perry_runtime::object::native_module_dispatch::dispatch_native_module_method";

/// One callable realm declaration and the functions a call of it runs.
/// A symbol that starts with `::` is the end of a path; one that starts
/// with `&` names a function and stands for every function whose address
/// that function takes (an export's getter holds its body in a local
/// static, and identical bodies are merged, so the body has no name of its
/// own to rely on).
pub struct Body {
    pub name: String,
    pub symbols: Vec<String>,
    /// The function-info record of the declaration (a global data symbol
    /// that holds the body's address): the body when its own symbol is in
    /// no archive, because the compiler merged it with an identical one.
    pub info: Option<String>,
    /// The module's dispatch function, for an export whose getter is in
    /// no archive.
    pub bucket: Option<String>,
    /// How the symbols were found (a census column).
    pub how: &'static str,
}

/// The export bodies the runtime's build script emits
/// (`object/native_module/static_exports_build.rs`), by the (module, key)
/// they dispatch: `js_native_export_<hex module>_<hex key>::body` calls
/// `nm_dispatch_<bucket>(ctx, module, key)` with the two names constant, so
/// its code holds what the module's dispatch function can run for that key
/// and no other arm's.
fn export_bodies() -> HashMap<(&'static str, &'static str), String> {
    use perry_dispatch::native_callable_exports::{
        canonical_native_callable_module, canonical_native_callable_property, export_bucket,
        export_value_symbol, CALLABLE_EXPORT_TABLE,
    };
    let mut out = HashMap::new();
    for &(module, properties) in CALLABLE_EXPORT_TABLE {
        if export_bucket(module).is_none() {
            continue;
        }
        for &property in properties {
            let Some(symbol) = export_value_symbol(module, property) else {
                continue;
            };
            let key = canonical_native_callable_property(module, property);
            let module = match canonical_native_callable_module(module, key) {
                "util/types" => "util.types",
                m => m,
            };
            out.entry((module, key)).or_insert(symbol);
        }
    }
    out
}

/// The module and key an export body dispatches, from its symbol.
fn export_of(spelling: &str) -> Option<(String, String)> {
    let rest = &spelling[spelling.find("js_native_export_")? + "js_native_export_".len()..];
    let rest = rest.split("::").next()?;
    let (module, key) = rest.split_once('_')?;
    let text = |hex: &str| {
        let bytes: Option<Vec<u8>> = (0..hex.len())
            .step_by(2)
            .map(|i| hex.get(i..i + 2).and_then(|b| u8::from_str_radix(b, 16).ok()))
            .collect();
        bytes.and_then(|b| String::from_utf8(b).ok())
    };
    Some((text(module)?, text(key)?))
}

/// Every callable realm declaration with the functions its call runs.
///
/// A declaration with a body of its own runs that body. A bound-method
/// member (a function of a native module namespace, of `process`, `console`,
/// `Buffer`) shares the sentinel body: its call enters [`ROUTER`] with the
/// namespace it captured and its key, and runs
/// - the function its effects-only declaration names (`effects_body`), and
/// - the export body of (module, key) when the build script emits one;
/// - with neither, the whole dispatch function of the module's bucket
///   (every arm: an upper bound);
/// - for a module without a bucket, the router alone (it answers from the
///   submodule registry through a function-value call, or throws).
///
/// The module is the `native_namespace` of the member's first capture, the
/// name the namespace object holds in its field 0 at run time.
pub fn bodies(manifest: &str) -> Result<Vec<Body>> {
    let v: serde_json::Value = serde_json::from_str(manifest).context("realm manifest JSON")?;
    let decls = v["declarations"]
        .as_array()
        .context("manifest has no declarations")?;
    let namespaces: HashMap<&str, &str> = decls
        .iter()
        .filter_map(|d| Some((d["name"].as_str()?, d["native_namespace"].as_str()?)))
        .collect();
    let exports = export_bodies();
    let mut out = Vec::new();
    for d in decls {
        let Some(body) = d.get("body").and_then(|b| b.as_str()) else {
            continue;
        };
        let name = d["name"]
            .as_str()
            .context("declaration without name")?
            .to_owned();
        let source = |v: &serde_json::Value| {
            v.as_str()
                .and_then(|s| s.strip_prefix("source:"))
                .map(str::to_owned)
        };
        if !body.ends_with("::BOUND_METHOD_FUNC_PTR") {
            let Some(symbol) = source(&d["function_info"]["source_symbol"]) else {
                continue;
            };
            out.push(Body {
                name,
                symbols: vec![symbol],
                info: d["info_symbol"].as_str().map(str::to_owned),
                bucket: None,
                how: "own body",
            });
            continue;
        }
        let capture = |i: usize, kind: &str| {
            d["captures"]
                .get(i)
                .filter(|c| c["kind"] == kind)
                .and_then(|c| c["value"].as_str())
        };
        let module = capture(0, "reference").and_then(|r| namespaces.get(r).copied());
        let (Some(module), Some(key)) = (module, capture(1, "bytes")) else {
            out.push(Body {
                name,
                symbols: Vec::new(),
                info: None,
                bucket: None,
                how: "a bound method of no module namespace",
            });
            continue;
        };
        let mut symbols = vec![ROUTER.to_owned()];
        let mut how = "router alone (no dispatch bucket)";
        if let Some(symbol) = d.get("effects_body").and_then(source) {
            symbols.push(symbol);
            how = "declared body";
        }
        let bucket = perry_dispatch::native_callable_exports::export_bucket(module)
            .map(|bucket| format!("::nm_dispatch_{bucket}"));
        if let Some(symbol) = exports.get(&(module, key)) {
            symbols.push(format!("&{symbol}"));
            how = "export body";
        } else if symbols.len() == 1 {
            if let Some(bucket) = &bucket {
                symbols.push(bucket.clone());
                how = "module dispatch";
            }
        }
        out.push(Body {
            name,
            symbols,
            info: None,
            bucket,
            how,
        });
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
    let short = |i: usize| g.label(i).chars().take(120).collect::<String>();
    let mut out = short(i);
    let mut steps = 0;
    while let Some(n) = next[i].filter(|&n| n != i) {
        // A step that is no call: `i` takes the address of `n`.
        let how = if g.succ[i].contains(&n) || !g.taken_by[n].contains(&i) {
            "->"
        } else {
            "-> (takes the address of)"
        };
        out.push_str(&format!("\n      {how} {}", short(n)));
        i = n;
        steps += 1;
        if steps > 40 {
            out.push_str("\n      -> ...");
            break;
        }
    }
    out
}

/// Checks every realm native against the graph. `indirect_pred` are the
/// delegated indirect edges of the GC rules (a hook wrapper's callees).
pub fn check(
    g: &Graph,
    indirect_pred: &[Vec<usize>],
    rules: &[Rule],
    natives: &[Body],
    births: &HashMap<String, Vec<String>>,
    undeclared: &[String],
) -> Result<Report> {
    let nn = g.nodes.len();
    let mut seed = vec![false; nn];
    let mut cut = vec![false; nn];
    let mut origin = vec![false; nn];
    let mut fills = vec![false; nn];
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
                    "fills" => fills[i] = true,
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
    let mut by_name: HashMap<&str, Vec<usize>> = HashMap::new();
    for i in 0..nn {
        for s in g.spellings(i) {
            by_name.entry(s).or_default().push(i);
        }
    }
    // A symbol given as the end of a path (`::nm_dispatch_fs`).
    let tails: Vec<(&str, usize)> = (0..nn)
        .flat_map(|i| g.spellings(i).into_iter().map(move |s| (s, i)))
        .filter(|(s, _)| s.contains("nm_dispatch_"))
        .collect();
    // The functions whose address an export's getter takes.
    let getters: HashMap<usize, &str> = natives
        .iter()
        .flat_map(|n| &n.symbols)
        .filter_map(|s| s.strip_prefix('&'))
        .flat_map(|s| by_name.get(s).into_iter().flatten().map(move |&i| (i, s)))
        .collect();
    let mut taken_from: HashMap<&str, Vec<usize>> = HashMap::new();
    for t in 0..nn {
        for c in &g.taken_by[t] {
            if let Some(getter) = getters.get(c) {
                taken_from.entry(getter).or_default().push(t);
            }
        }
    }
    let find = |symbol: &str| -> Vec<usize> {
        if let Some(getter) = symbol.strip_prefix('&') {
            taken_from.get(getter).cloned().unwrap_or_default()
        } else if symbol.starts_with("::") {
            tails
                .iter()
                .filter(|(s, _)| s.ends_with(symbol))
                .map(|&(_, i)| i)
                .collect()
        } else {
            by_name.get(symbol).cloned().unwrap_or_default()
        }
    };
    // Per native: the nodes of its symbols; `None`: one is in no archive.
    let found: Vec<Option<Vec<usize>>> = natives
        .iter()
        .map(|n| {
            let per: Vec<Vec<usize>> = n
                .symbols
                .iter()
                .map(|s| {
                    let mut nodes = find(s);
                    if nodes.is_empty() && s.starts_with('&') {
                        nodes = n.bucket.as_deref().map(&find).unwrap_or_default();
                    }
                    if nodes.is_empty() {
                        let info = n.info.as_ref().and_then(|i| g.held_by_symbol.get(i));
                        nodes = info.cloned().unwrap_or_default();
                    }
                    nodes
                })
                .collect();
            (!per.is_empty() && per.iter().all(|p| !p.is_empty()))
                .then(|| per.into_iter().flatten().collect())
        })
        .collect();
    let mut native_node = vec![false; nn];
    for &i in found.iter().flatten().flatten() {
        native_node[i] = true;
    }
    let export_body = |i: usize| {
        g.spellings(i)
            .iter()
            .any(|s| s.contains("::static_exports::") && export_of(s).is_some())
    };
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
    // Who can make `t` run: its callers, the wrapper of the slot it is
    // delegated to, and whoever takes its address (in code, or by naming
    // data that holds it): that function runs it or hands it to what will
    // (a completion it schedules, a callback it registers). A `fills`
    // function only stores the address in a slot.
    //
    // Not followed: the takers of a realm native's body or of an export
    // body. Its address is held to make its function value; a call of that
    // value is the native, which is checked by name below (a function-value
    // call is a cut). Nor the takers of a function with an `origin` rule:
    // the rule says what makes it run.
    let held_as_value: Vec<bool> = (0..nn)
        .map(|i| native_node[i] || origin[i] || export_body(i))
        .collect();
    let (fills, held_as_value) = (&fills, &held_as_value);
    let takers = |t: usize| {
        g.taken_by[t]
            .iter()
            .filter(move |&&c| !fills[c] && !held_as_value[t])
    };
    while let Some(t) = work.pop_front() {
        for &c in g.pred[t].iter().chain(&indirect_pred[t]).chain(takers(t)) {
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
    let mut roots = Vec::new();
    let mut origins = (0usize, 0usize);
    for i in 0..nn {
        if next[i].is_none() {
            continue;
        }
        let called = g.pred[i]
            .iter()
            .chain(&indirect_pred[i])
            .chain(takers(i))
            .any(|&c| c != i && next[c].is_some());
        // Reached in a way the walk cannot attribute: data no code of the
        // archives names holds its address, or an install stored it in a
        // slot no rule delegates to a wrapper.
        let stored = indirect_pred[i].is_empty() && g.taken_by[i].iter().any(|&c| fills[c]);
        let by_pointer =
            g.loose[i] || stored || (held_as_value[i] && !g.taken_by[i].is_empty());
        if called && !by_pointer {
            continue;
        }
        let spellings = g.spellings(i);
        // An export body the realm declares no member for: the value of
        // export `key` of `module`, which a program gets only by naming the
        // module. Covered when the module is of the family (or is one the
        // table does not classify: every family is live then).
        let export_of_family = spellings.iter().any(|s| {
            s.contains("::static_exports::")
                && export_of(s).is_some_and(|(module, _)| {
                    families::module_families(&module)
                        .is_none_or(|f| families::close_requires(f) & families::family_bit(FAMILY) != 0)
                })
        });
        let kind =
            if native_node[i] {
                "native"
            } else if export_of_family {
                "export body of a family module"
            } else if origin[i] {
                origins.0 += 1;
                "by pointer, origin stated"
            } else if !stored
                && spellings
                    .iter()
                    .any(|s| !s.contains("::") && !s.starts_with('_'))
            {
                // An entry point compiled code or a native library calls by
                // name: what the program's code may call is the fact's node
                // classification, not this check. (One an install stores in
                // a slot is called through the slot: it needs its origin.)
                "exported"
            } else {
                origins.1 += 1;
                report.failures += 1;
                report.lines.push(format!(
                "{}: FAIL: it can reach a byte-cell birth and is reached only through a pointer \
                 ({}), and no origin rule says what schedules it\n      {}",
                g.label(i),
                match (g.loose[i], stored) {
                    (true, _) => "data no runtime code names holds its address",
                    (_, true) => "an install stores its address in a slot",
                    _ => "no caller, and no code takes its address",
                },
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
        .push_str("name\tsymbols\thow\tfound\treaches\tverdict\n");
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut unchecked = 0usize;
    for (n, nodes) in natives.iter().zip(&found) {
        let nodes = nodes.as_ref();
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
            "{}\t{}\t{}\t{}\t{}\t{}\n",
            n.name,
            n.symbols.join(" "),
            n.how,
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
            "birth" | "in-hand" | "origin" | "before-claim" | "fills"
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
