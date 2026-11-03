use crate::instructions::{rx, Format};
use crate::rules::Rule;
use crate::table::{native_stems, perry_member, Class, Entry, Table};
use anyhow::{bail, Result};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    path::Path,
};

#[derive(Debug)]
pub struct Node {
    pub obj: std::sync::Arc<str>,
    pub sec: std::sync::Arc<str>,
    pub addr: u64,
    pub names: Vec<String>,
    pub indirect_calls: usize,
    pub indirect_jmps: usize,
    pub calls: BTreeSet<String>,
    pub externs: BTreeSet<String>,
    pub idioms: BTreeSet<String>,
    pub sites: Vec<String>,
    pub switches: usize,
    pub(crate) fmt: Format,
}
impl Node {
    pub(crate) fn new(
        obj: std::sync::Arc<str>,
        sec: std::sync::Arc<str>,
        addr: u64,
        fmt: Format,
    ) -> Self {
        Self {
            obj,
            sec,
            addr,
            fmt,
            names: Vec::new(),
            indirect_calls: 0,
            indirect_jmps: 0,
            calls: BTreeSet::new(),
            externs: BTreeSet::new(),
            idioms: BTreeSet::new(),
            sites: Vec::new(),
            switches: 0,
        }
    }
}
pub(crate) fn temp_label(name: &str) -> bool {
    crate::instructions::starts(
        name,
        &["ltmp", "Ltmp", ".L", "L_", "l_", "lCPI", "LCPI", "$"],
    ) || rx!(r"^\.[A-Za-z_]+(\$[A-Za-z0-9_.]*)?$").is_match(name)
}
pub(crate) fn crate_of(obj: &str) -> Option<&str> {
    let member = obj.rsplit(['/', '\\']).next()?;
    if !member.contains(".rcgu.") {
        return None;
    }
    let parts: Vec<_> = member.split('.').collect();
    if parts.len() < 3 {
        return None;
    }
    Some(if let Some((head, hash)) = parts[1].rsplit_once('-') {
        if hash.len() == 16
            && hash
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            head
        } else {
            parts[1]
        }
    } else {
        parts[1]
    })
}
pub struct Graph {
    pub nodes: Vec<Node>,
    pub succ: Vec<Vec<usize>>,
    pub pred: Vec<Vec<usize>>,
    pub by_global: HashMap<String, Vec<usize>>,
    pub dem: HashMap<String, String>,
    pub edge_count: usize,
}
fn plain_llvm(s: &str) -> &str {
    rx!(r"(\s*\(\.llvm\.\d+\)|\.llvm\.\d+)$")
        .find(s)
        .map_or(s, |m| &s[..m.start()])
}

impl Graph {
    pub(crate) fn new(
        mut nodes: Vec<Node>,
        globals: HashSet<(std::sync::Arc<str>, String)>,
    ) -> Result<Self> {
        let mut local: HashMap<(&str, &str), Vec<usize>> = HashMap::new();
        let mut sections: HashMap<(&str, &str), Vec<usize>> = HashMap::new();
        let mut by_global: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, n) in nodes.iter().enumerate() {
            sections
                .entry((n.obj.as_ref(), n.sec.as_ref()))
                .or_default()
                .push(i);
            for name in &n.names {
                local
                    .entry((n.obj.as_ref(), name.as_str()))
                    .or_default()
                    .push(i);
                if globals.contains(&(n.obj.clone(), name.clone())) {
                    by_global.entry(name.clone()).or_default().push(i);
                }
            }
        }
        let mut succ = vec![Vec::new(); nodes.len()];
        let mut externs = vec![BTreeSet::new(); nodes.len()];
        for (i, n) in nodes.iter().enumerate() {
            let mut targets: BTreeSet<usize> = BTreeSet::new();
            for t in &n.calls {
                if let Some(ts) = local.get(&(n.obj.as_ref(), t.as_str())) {
                    targets.extend(ts.iter().copied());
                    continue;
                }
                if let Some(ts) = by_global.get(t) {
                    targets.extend(ts.iter().copied());
                    continue;
                }
                let macho_sec = format!("__TEXT,{t}");
                if let Some(ts) = sections
                    .get(&(n.obj.as_ref(), t.as_str()))
                    .or_else(|| sections.get(&(n.obj.as_ref(), macho_sec.as_str())))
                {
                    targets.extend(ts.iter().copied());
                    continue;
                }
                externs[i].insert(t.clone());
            }
            targets.remove(&i);
            succ[i] = targets.into_iter().collect();
        }
        drop(local);
        drop(sections);
        for (n, ex) in nodes.iter_mut().zip(externs) {
            n.externs = ex;
        }
        let mut pred = vec![Vec::new(); nodes.len()];
        for (i, ts) in succ.iter().enumerate() {
            for &t in ts {
                pred[t].push(i);
            }
        }
        let mut dem = HashMap::new();
        for n in &nodes {
            for s in n.names.iter().chain(&n.externs) {
                dem.entry(s.clone()).or_insert_with(|| {
                    rustc_demangle::try_demangle(s)
                        .map(|d| format!("{d:#}"))
                        .unwrap_or_else(|_| s.clone())
                });
            }
        }
        let edge_count = succ.iter().map(Vec::len).sum();
        Ok(Self {
            nodes,
            succ,
            pred,
            by_global,
            dem,
            edge_count,
        })
    }
    pub fn label(&self, i: usize) -> String {
        let n = &self.nodes[i];
        let real: Vec<_> = n.names.iter().filter(|s| !temp_label(s)).collect();
        let real = if real.is_empty() {
            n.names.iter().collect()
        } else {
            real
        };
        if real.is_empty() {
            return format!("{}+{:#x}", n.sec, n.addr);
        }
        real.into_iter()
            .map(|s| self.dem.get(s).unwrap_or(s).as_str())
            .collect::<Vec<_>>()
            .join(" | ")
    }
    pub fn spellings(&self, i: usize) -> Vec<&str> {
        let mut out = Vec::new();
        for s in &self.nodes[i].names {
            out.push(s.as_str());
            if let Some(d) = self.dem.get(s).filter(|d| *d != s) {
                out.push(plain_llvm(d));
            }
        }
        out
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    L2,
    L2b,
    L2bThrow,
    Throws,
}
impl Mode {
    pub const ALL: [Self; 4] = [Self::L2, Self::L2b, Self::L2bThrow, Self::Throws];
    pub fn name(self) -> &'static str {
        match self {
            Self::L2 => "L2",
            Self::L2b => "L2b",
            Self::L2bThrow => "L2b_throw",
            Self::Throws => "throws",
        }
    }
    fn index(self) -> usize {
        match self {
            Self::L2 => 0,
            Self::L2b => 1,
            Self::L2bThrow => 2,
            Self::Throws => 3,
        }
    }
}
#[derive(Default, Debug)]
pub struct Stats {
    pub nodes: usize,
    pub edges: usize,
    pub indirect_call_nodes: usize,
    pub indirect_jump_nodes: usize,
    pub exempted_indirect_nodes: usize,
    pub trusted_indirect_nodes: usize,
    pub named_seed_nodes: BTreeMap<String, usize>,
    pub unresolved_externs: BTreeSet<String>,
    pub dead_rules: Vec<String>,
    pub forbidden_calls: Vec<String>,
    pub unused_exemptions: Vec<String>,
    pub shadowed_demoted: Vec<String>,
}
#[derive(Default)]
pub struct Classification {
    pub table: Table,
    pub node_of: BTreeMap<String, usize>,
    /// Every node that defines an exported symbol. A symbol defined more than
    /// once in the link set (the stdlib archive bundles a runtime copy) is
    /// classified by the join of its definitions.
    pub defs: BTreeMap<String, Vec<usize>>,
    pub stats: Stats,
    reason: Vec<Vec<Option<String>>>,
    witness: Vec<Vec<Option<usize>>>,
}
impl Classification {
    pub fn witness_path(&self, g: &Graph, name: &str, mode: Mode) -> Vec<String> {
        let Some(&first) = self.node_of.get(name) else {
            return vec![format!("{name}: not an exported symbol of these archives")];
        };
        let m = mode.index();
        // Of several definitions, explain through one that reaches a seed.
        let i = self
            .defs
            .get(name)
            .and_then(|d| d.iter().copied().find(|&i| self.reason[m][i].is_some()))
            .unwrap_or(first);
        if self.reason[m][i].is_none() {
            return vec![format!("{name}: reaches no seed in mode {}", mode.name())];
        }
        let mut i = Some(i);
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        while let Some(n) = i {
            if !seen.insert(n) {
                break;
            }
            let mut why = self.reason[m][n].as_deref().unwrap_or("?").to_owned();
            if why.starts_with("indirect") {
                if let Some(site) = g.nodes[n].sites.first() {
                    why.push_str(" @");
                    why.push_str(site);
                }
            }
            out.push(format!(
                "{why:>28}  {}",
                g.label(n).chars().take(200).collect::<String>()
            ));
            i = self.witness[m][n];
        }
        out
    }
}
fn match_any<'a>(rules: &[&'a Rule], spell: &[&str]) -> Option<&'a Rule> {
    rules
        .iter()
        .copied()
        .find(|r| spell.iter().any(|s| r.matches(s)))
}
fn match_indirect<'a>(rules: &[&'a Rule], g: &Graph, i: usize, spell: &[&str]) -> Option<&'a Rule> {
    rules.iter().copied().find(|r| {
        if r.kind != "indirect-all" {
            return spell.iter().any(|s| r.matches(s));
        }
        let names: Vec<_> = g.nodes[i].names.iter().filter(|s| !temp_label(s)).collect();
        !names.is_empty()
            && names
                .iter()
                .all(|s| r.matches(s) || r.matches(plain_llvm(g.dem.get(*s).unwrap_or(s))))
    })
}
// Independent rule matches are evaluated in parallel; the returned order
// stays node order, including the tie-breaking of aliases and witnesses.
fn parallel_map<T: Send, F: Fn(usize) -> T + Sync>(nn: usize, f: F) -> Vec<T> {
    let workers = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(8);
    let chunk = nn.div_ceil(workers).max(1);
    std::thread::scope(|scope| {
        let f = &f;
        let mut handles = Vec::new();
        for start in (0..nn).step_by(chunk) {
            handles.push(
                scope.spawn(move || (start..(start + chunk).min(nn)).map(f).collect::<Vec<_>>()),
            );
        }
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("rule matching thread"))
            .collect()
    })
}
fn matching_nodes(g: &Graph, spell: &[Vec<&str>], rule: &Rule) -> Vec<usize> {
    parallel_map(g.nodes.len(), |i| spell[i].iter().any(|s| rule.matches(s)))
        .into_iter()
        .enumerate()
        .filter_map(|(i, hit)| hit.then_some(i))
        .collect()
}
pub fn classify(g: &Graph, rules: &[Rule], root: Option<&Path>) -> Result<Classification> {
    let nn = g.nodes.len();
    let spell = parallel_map(nn, |i| g.spellings(i));
    let kinds = |kind: &str| rules.iter().filter(|r| r.kind == kind).collect::<Vec<_>>();
    let named_kinds = [
        "poll",
        "js",
        "collector",
        "throw",
        "panic",
        "teardown",
        "nothrow",
    ];
    let mut named = vec![None; nn];
    let mut hits = HashMap::<usize, usize>::new();
    let named_rules: Vec<_> = named_kinds.iter().map(|kind| kinds(kind)).collect();
    let matches = parallel_map(nn, |i| {
        named_rules
            .iter()
            .filter_map(|rs| match_any(rs, &spell[i]))
            .collect::<Vec<_>>()
    });
    for (i, matches) in matches.into_iter().enumerate() {
        for r in matches {
            *hits.entry(r.line).or_default() += 1;
            if named[i].is_none() {
                named[i] = Some(r);
            }
        }
    }
    let mut ind_rules = kinds("indirect");
    ind_rules.extend(kinds("indirect-all"));
    let indirect_matches = parallel_map(nn, |i| {
        let n = &g.nodes[i];
        if n.indirect_calls != 0 || n.indirect_jmps != 0 {
            match_indirect(&ind_rules, g, i, &spell[i])
        } else {
            None
        }
    });
    let trust_rules = kinds("trusted");
    let ext_rules = kinds("extern");
    let mut exempt = HashMap::new();
    let mut trusted = HashSet::new();
    let mut extern_ok = HashSet::new();
    let mut trust: HashMap<&str, bool> = HashMap::new();
    for (i, n) in g.nodes.iter().enumerate() {
        if n.indirect_calls != 0 || n.indirect_jmps != 0 {
            if let Some(r) = indirect_matches[i] {
                exempt.insert(i, r);
                *hits.entry(r.line).or_default() += 1;
            } else if let Some(c) = crate_of(&n.obj) {
                let ok = *trust.entry(c).or_insert_with(|| {
                    if let Some(r) = match_any(&trust_rules, &[c]) {
                        *hits.entry(r.line).or_default() += 1;
                        true
                    } else {
                        false
                    }
                });
                if ok {
                    trusted.insert(i);
                }
            }
        }
        for e in &n.externs {
            if !extern_ok.contains(e) {
                if let Some(r) = match_any(
                    &ext_rules,
                    &[e.as_str(), g.dem.get(e).unwrap_or(e).as_str()],
                ) {
                    extern_ok.insert(e.clone());
                    *hits.entry(r.line).or_default() += 1;
                }
            }
        }
    }
    let mut extra_pred = vec![Vec::new(); nn];
    let mut ordered: Vec<_> = exempt.iter().collect();
    ordered.sort_by_key(|(i, _)| **i);
    let target_matches: HashMap<_, _> = ind_rules
        .iter()
        .filter_map(|r| {
            r.targets
                .as_ref()
                .map(|t| (r.line, matching_nodes(g, &spell, t)))
        })
        .collect();
    for (&i, rule) in &ordered {
        if let Some(targets) = target_matches.get(&rule.line) {
            if targets.is_empty() {
                bail!(
                    "seeds.txt:{}: delegated callback pattern matched no functions for {}: {}",
                    rule.line,
                    g.label(i),
                    rule.targets.as_ref().unwrap().pattern
                );
            }
            for &t in targets {
                if t != i {
                    extra_pred[t].push(i);
                }
            }
        }
    }
    let mut forbidden = Vec::new();
    for r in kinds("forbid") {
        for (i, n) in g.nodes.iter().enumerate() {
            for t in &n.calls {
                if r.matches(t) || r.matches(g.dem.get(t).unwrap_or(t)) {
                    forbidden.push(format!(
                        "seeds.txt:{}: {} calls {t}",
                        r.line,
                        g.label(i).chars().take(120).collect::<String>()
                    ));
                    *hits.entry(r.line).or_default() += 1;
                }
            }
        }
    }
    let closures = kinds("closures");
    if !closures.is_empty() {
        let mut prefix: HashMap<&str, Vec<usize>> = HashMap::new();
        for (t, spells) in spell.iter().enumerate() {
            for s in spells {
                if let Some(k) = s.find("::{closure#").filter(|k| *k > 0) {
                    prefix.entry(&s[..k]).or_default().push(t);
                }
            }
        }
        for (i, spellings) in spell.iter().enumerate() {
            if let Some(r) = match_any(&closures, spellings) {
                *hits.entry(r.line).or_default() += 1;
                for s in spellings {
                    if let Some(ts) = prefix.get(*s) {
                        for &t in ts {
                            if t != i {
                                extra_pred[t].push(i);
                            }
                        }
                    }
                }
            }
        }
    }
    let mut idiom_rules: Vec<(&str, &Rule)> = Vec::new();
    for r in kinds("idiom") {
        if let Some((_, old)) = idiom_rules.iter_mut().find(|(name, _)| *name == r.pattern) {
            *old = r;
        } else {
            idiom_rules.push((&r.pattern, r));
        }
    }
    let mut unknown_idiom = vec![None; nn];
    for &(id, r) in &idiom_rules {
        let targets = r
            .targets
            .as_ref()
            .map(|target| matching_nodes(g, &spell, target))
            .unwrap_or_default();
        for (i, n) in g.nodes.iter().enumerate() {
            if n.idioms.contains(id) {
                *hits.entry(r.line).or_default() += 1;
                for &t in &targets {
                    if t != i {
                        extra_pred[t].push(i);
                    }
                }
            }
        }
    }
    for (i, n) in g.nodes.iter().enumerate() {
        for id in &n.idioms {
            if !idiom_rules.iter().any(|(name, _)| *name == id) {
                unknown_idiom[i] = Some(id.as_str());
            }
        }
    }
    for (i, n) in g.nodes.iter().enumerate() {
        if (n.indirect_calls != 0 || n.indirect_jmps != 0)
            && !exempt.contains_key(&i)
            && !trusted.contains(&i)
            && n.names.iter().any(|s| s.starts_with("OUTLINED_FUNCTION_"))
            && !g.pred[i].is_empty()
            && g.pred[i].iter().all(|&c| {
                match_indirect(&ind_rules, g, c, &spell[c]).is_some()
                    || crate_of(&g.nodes[c].obj)
                        .is_some_and(|k| match_any(&trust_rules, &[k]).is_some())
            })
        {
            trusted.insert(i);
        }
    }
    let unwind = kinds("unwind");
    let mut structural = vec![None; nn];
    for (i, n) in g.nodes.iter().enumerate() {
        for e in &n.externs {
            if let Some(r) = match_any(&unwind, &[e.as_str(), g.dem.get(e).unwrap_or(e).as_str()]) {
                *hits.entry(r.line).or_default() += 1;
                if structural[i].is_none() {
                    structural[i] = Some(format!("unwind:{e}"));
                }
            }
        }
        if structural[i].is_none() {
            structural[i] = if let Some(id) = unknown_idiom[i] {
                Some(format!("idiom_without_rule:{id}"))
            } else if (n.indirect_calls != 0 || n.indirect_jmps != 0)
                && !exempt.contains_key(&i)
                && !trusted.contains(&i)
            {
                Some(
                    if n.indirect_calls != 0 {
                        "indirect_call"
                    } else {
                        "indirect_jump"
                    }
                    .into(),
                )
            } else {
                n.externs
                    .iter()
                    .find(|e| !extern_ok.contains(*e))
                    .map(|e| format!("extern:{e}"))
            };
        }
    }
    let mut reason = Vec::new();
    let mut witness = Vec::new();
    for mode in Mode::ALL {
        let cut = |i: usize| {
            named[i].is_some_and(|r| {
                ["panic", "teardown"].contains(&r.kind.as_str())
                    || mode == Mode::Throws
                        && ["collector", "poll", "nothrow"].contains(&r.kind.as_str())
                    || [Mode::L2b, Mode::L2bThrow].contains(&mode) && r.kind == "collector"
                    || mode == Mode::L2bThrow && r.kind == "throw"
            })
        };
        let mut bad = vec![None; nn];
        let mut nxt = vec![None; nn];
        let mut work = VecDeque::new();
        for i in 0..nn {
            if cut(i) {
                continue;
            }
            let mut s = structural[i].clone();
            if let Some(r) = named[i] {
                let named_seed = if mode == Mode::Throws {
                    ["js", "throw"].contains(&r.kind.as_str())
                } else {
                    ["poll", "js"].contains(&r.kind.as_str())
                };
                if named_seed {
                    s = Some(format!("{}:{}", r.kind, r.pattern));
                }
                if s.is_none()
                    && (mode == Mode::L2 && r.kind == "collector"
                        || [Mode::L2, Mode::L2b].contains(&mode) && r.kind == "throw")
                {
                    s = Some(format!("{}:{}", r.kind, r.pattern));
                }
            }
            if let Some(s) = s {
                bad[i] = Some(s);
                work.push_back(i);
            }
        }
        while let Some(t) = work.pop_front() {
            for &c in g.pred[t].iter().chain(&extra_pred[t]) {
                if bad[c].is_some() || cut(c) {
                    continue;
                }
                bad[c] = Some("via".into());
                nxt[c] = Some(t);
                work.push_back(c);
            }
        }
        reason.push(bad);
        witness.push(nxt);
    }
    let stems = if let Some(root) = root {
        native_stems(root)?
    } else {
        HashSet::new()
    };
    let mut node_of = BTreeMap::new();
    let mut defs: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, n) in g.nodes.iter().enumerate() {
        for name in &n.names {
            if g.by_global.get(name).is_some_and(|ns| ns.contains(&i))
                && (root.is_none() || perry_member(&n.obj, name, &stems))
            {
                node_of.entry(name.clone()).or_insert(i);
                let d = defs.entry(name.clone()).or_default();
                if !d.contains(&i) {
                    d.push(i);
                }
            }
        }
    }
    let entry_of = |i: usize| {
        let class = if reason[0][i].is_none() {
            Class::Leaf
        } else if reason[1][i].is_none() {
            Class::AllocOnly
        } else if reason[2][i].is_none() {
            Class::ThrowOnly
        } else {
            Class::Reenters
        };
        Entry {
            class,
            throws: reason[3][i].is_some(),
        }
    };
    // The join of the definitions: the weaker class and the OR of throws, so
    // the row holds whichever definition the linker keeps.
    let table = defs
        .iter()
        .map(|(name, nodes)| {
            let mut row = entry_of(nodes[0]);
            for &i in &nodes[1..] {
                let e = entry_of(i);
                row.class = row.class.max(e.class);
                row.throws |= e.throws;
            }
            (name.clone(), row)
        })
        .collect();
    let mut named_seed_nodes = BTreeMap::new();
    for r in named.iter().flatten() {
        *named_seed_nodes.entry(r.kind.clone()).or_default() += 1;
    }
    let stats = Stats {
        nodes: nn,
        edges: g.edge_count,
        indirect_call_nodes: g.nodes.iter().filter(|n| n.indirect_calls != 0).count(),
        indirect_jump_nodes: g.nodes.iter().filter(|n| n.indirect_jmps != 0).count(),
        exempted_indirect_nodes: exempt.len(),
        trusted_indirect_nodes: trusted.len(),
        named_seed_nodes,
        unresolved_externs: g
            .nodes
            .iter()
            .flat_map(|n| &n.externs)
            .filter(|e| !extern_ok.contains(*e))
            .cloned()
            .collect(),
        dead_rules: rules
            .iter()
            .filter(|r| named_kinds.contains(&r.kind.as_str()) && !hits.contains_key(&r.line))
            .map(Rule::description)
            .collect(),
        forbidden_calls: forbidden,
        unused_exemptions: rules
            .iter()
            .filter(|r| {
                ["indirect", "extern", "unwind"].contains(&r.kind.as_str())
                    && !hits.contains_key(&r.line)
            })
            .map(Rule::description)
            .collect(),
        shadowed_demoted: Vec::new(),
    };
    Ok(Classification {
        table,
        node_of,
        defs,
        stats,
        reason,
        witness,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn unicode_symbol_names_survive_llvm_suffix_normalization() {
        let name = "perry_fn_module_ts__gr.uC3.uBC.uC3.u9Fe.u24m.uC3.uA9thode";
        assert_eq!(super::plain_llvm(name), name);
        assert_eq!(super::plain_llvm(&format!("{name}.llvm.12345")), name);
        assert_eq!(super::plain_llvm(&format!("{name} (.llvm.12345)")), name);
        let table = crate::parse_table(&format!("{name}\tLeaf\tno\n"), "unicode").unwrap();
        assert!(table.contains_key(name));
    }

    use super::*;
    #[test]
    fn indirect_all_requires_every_callable_alias() {
        let rules = crate::load_rules(
            "indirect-all regex ^audited_ -- only audited initializer aliases are native leaves\n",
        )
        .unwrap();
        let rules: Vec<_> = rules.iter().collect();
        let mut node = Node::new("fixture.o".into(), ".text".into(), 0, Format::Elf);
        node.names = vec!["audited_initializer".into()];
        let mut graph = Graph::new(vec![node], HashSet::new()).unwrap();
        let admitted = |g: &Graph| match_indirect(&rules, g, 0, &g.spellings(0)).is_some();
        assert!(admitted(&graph));
        graph.nodes[0].names.push("unrelated_initializer".into());
        assert!(!admitted(&graph));
        for sec in [".text$unlikely", ".text", ".rdata$zzz", ".xdata$x"] {
            graph.nodes[0].names = vec!["audited_initializer".into(), sec.into()];
            assert!(admitted(&graph));
            assert!(temp_label(sec));
        }
        for names in [
            vec![".text$unlikely"],
            vec!["unrelated_initializer", ".text$unlikely"],
            vec![
                "audited_initializer",
                "_ZN4core3ptr$u20$x17h0123456789abcdefE",
            ],
        ] {
            graph.nodes[0].names = names.into_iter().map(str::to_owned).collect();
            assert!(!admitted(&graph));
        }
        for name in [
            "_ZN4core3ptr$u20$drop$LT$T$GT$17h0123456789abcdefE",
            "foo$bar",
            "text$unlikely",
        ] {
            assert!(!temp_label(name));
        }
        let stems = HashSet::from(["hot_text".to_owned()]);
        assert!(!perry_member(
            "8a1b2c3d-mimalloc.o",
            "js_back_hot_text",
            &stems
        ));
    }
}
