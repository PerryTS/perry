//! Caller-owned view of the realm table. Only construction parses facts.
use crate::realm_manifest::{RealmEntry, RealmKind, RealmOwnValue};
use perry_runtime_manifest::abi as perry_abi;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug)]
pub struct RealmIndex {
    rows: Vec<IndexedRow>,
    by_name: BTreeMap<&'static str, Vec<usize>>,
    primitive_members: BTreeMap<&'static str, Vec<usize>>,
    by_module: BTreeMap<&'static str, Vec<usize>>,
}
#[derive(Debug)]
struct IndexedRow {
    entry: RealmEntry,
    facts: serde_json::Value,
    structure: BTreeSet<String>,
}
#[derive(Clone, Copy)]
pub struct Row<'a>(&'a IndexedRow);
impl<'a> Row<'a> {
    pub fn kind(self) -> RealmKind {
        self.0.entry.kind
    }
    pub fn prototype(self) -> Option<&'static str> {
        self.0.entry.proto
    }
    pub fn structure_has(self, birth: &str) -> bool {
        self.0.structure.contains(birth)
    }
    pub fn has_constructor(self) -> bool {
        self.0
            .facts
            .get("constructor")
            .is_some_and(|v| !v.is_null())
    }
    /// A constructor birth denotes an instance, whose chain starts here.
    pub fn instance_prototype(self) -> Option<&'static str> {
        self.has_constructor()
            .then(|| {
                self.members()
                    .find(|(key, _, _)| *key == "prototype")
                    .map(|(_, _, value)| value)
            })
            .flatten()
    }
    pub fn member_enumerable(self, key: &str, symbol: bool) -> bool {
        self.0
            .facts
            .get("properties")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .any(|p| {
                p["key"].as_str() == Some(key)
                    && p["symbol"].as_bool().unwrap_or(false) == symbol
                    && p["attributes"].as_u64().unwrap_or(0) & 2 != 0
            })
    }
    pub fn runtime_layout_holder(self) -> bool {
        self.0
            .facts
            .pointer("/header/gc_type")
            .and_then(|v| v.as_u64())
            .is_some_and(|kind| perry_abi::gc_type_is_array!(kind as u8))
            && self
                .0
                .entry
                .births
                .iter()
                .any(|b| b.starts_with("%Layout:"))
    }
    pub fn argument_mode(self, mode: &str) -> bool {
        self.0
            .facts
            .get("flags")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .any(|flag| {
                flag.as_str()
                    .is_some_and(|value| value.strip_suffix("_arguments") == Some(mode))
            })
    }
    pub fn runtime_spelled_key(self, key: &str) -> bool {
        self.0
            .facts
            .get("flags")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .any(|flag| {
                flag.as_str()
                    .and_then(|v| v.strip_prefix("runtime_key:"))
                    .is_some_and(|v| v == key)
            })
    }
    pub fn name(self) -> &'static str {
        self.0.entry.name
    }
    pub fn entry(self) -> &'a RealmEntry {
        &self.0.entry
    }
    pub fn facts(self) -> &'a serde_json::Value {
        &self.0.facts
    }
    pub fn calls(self) -> impl Iterator<Item = &'a str> {
        self.0
            .facts
            .get("calls")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str())
    }
    pub fn writes(self) -> impl Iterator<Item = (&'a str, &'a str, &'a str)> {
        self.0
            .facts
            .get("writes")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter_map(|v| {
                Some((
                    v.get("holder")?.as_str()?,
                    v.get("key")?.as_str()?,
                    v.get("value")?.as_str()?,
                ))
            })
    }
    pub fn operation(self) -> Option<&'a str> {
        self.0.facts.get("operation").and_then(|v| v.as_str())
    }
    pub fn demands(self) -> impl Iterator<Item = &'a str> {
        self.0
            .facts
            .get("demands")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str())
    }
    pub fn body(self) -> Option<&'a str> {
        self.0
            .facts
            .get("effects_body")
            .and_then(|v| v.as_str())
            .and_then(|body| body.strip_prefix("source:"))
            .or_else(|| self.0.facts.get("body").and_then(|v| v.as_str()))
    }
    pub fn births(self) -> impl Iterator<Item = &'static str> {
        self.0.entry.births.iter().copied()
    }
    pub fn members(self) -> impl Iterator<Item = (&'static str, bool, &'static str)> + 'a {
        self.0.entry.own.iter().flat_map(|p| {
            let values = match p.value {
                RealmOwnValue::Reference(v) => [Some(v), None],
                RealmOwnValue::Accessor { get, set } => [get, set],
                _ => [None, None],
            };
            values
                .into_iter()
                .flatten()
                .map(move |v| (p.key, p.symbol, v))
        })
    }
}
fn parse_facts(facts: &str) -> serde_json::Value {
    #[cfg(test)]
    FACTS_PARSES.with(|n| n.set(n.get() + 1));
    serde_json::from_str(facts).expect("valid generated realm facts")
}
#[cfg(test)]
thread_local! {
    static FACTS_PARSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static INDEX_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
impl RealmIndex {
    pub fn new(entries: &[RealmEntry]) -> Self {
        #[cfg(test)]
        INDEX_BUILDS.with(|n| n.set(n.get() + 1));
        let mut result = Self {
            rows: Vec::with_capacity(entries.len()),
            by_name: BTreeMap::new(),
            by_module: BTreeMap::new(),
            primitive_members: Default::default(),
        };
        for entry in entries {
            let i = result.rows.len();
            result.by_name.entry(entry.name).or_default().push(i);
            if let Some(module) = entry.module {
                result.by_module.entry(module).or_default().push(i);
            }
            let facts = parse_facts(entry.facts);
            let mut structure = BTreeSet::new();
            structure.extend(entry.proto.map(str::to_owned));

            for value in facts
                .get("captures")
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
            {
                if matches!(
                    value.get("kind").and_then(|v| v.as_str()),
                    Some("reference" | "raw_reference")
                ) {
                    if let Some(v) = value.get("value").and_then(|v| v.as_str()) {
                        structure.insert(v.into());
                    }
                }
            }
            if let Some((kind, _)) = entry.name.split_once(".prototype") {
                if member_of_primitive(entry.name, kind, None) {
                    result.primitive_members.entry(kind).or_default().push(i);
                }
            }
            result.rows.push(IndexedRow {
                entry: *entry,
                facts,
                structure,
            });
        }
        let storage = result
            .rows
            .iter()
            .filter_map(|r| {
                let owners = r.facts.pointer("/storage/owners")?.as_array()?;
                Some((
                    r.entry.name,
                    owners
                        .iter()
                        .filter_map(|v| v.as_str())
                        .map(str::to_owned)
                        .collect::<Vec<_>>(),
                ))
            })
            .collect::<Vec<_>>();
        for (layout, owners) in storage {
            for row in &mut result.rows {
                if owners.iter().any(|owner| owner == row.entry.name) {
                    row.structure.insert(layout.into());
                }
            }
        }
        result
    }
    pub fn operation(&self, symbol: &str) -> Option<Row<'_>> {
        self.rows()
            .find(|r| r.facts().get("operation").and_then(|v| v.as_str()) == Some(symbol))
    }
    pub fn bodies<'a>(&'a self, symbol: &'a str) -> impl Iterator<Item = Row<'a>> + 'a {
        self.rows().filter(move |row| {
            row.operation()
                .or_else(|| row.body())
                .is_some_and(|body| body.rsplit("::").next() == Some(symbol))
        })
    }
    pub fn kind_birth(&self, birth: &str) -> bool {
        self.last(birth).is_some_and(|row| row.has_constructor())
    }
    pub fn rows(&self) -> impl Iterator<Item = Row<'_>> {
        self.rows.iter().map(Row)
    }
    pub fn has(&self, name: &str) -> bool {
        self.by_name.contains_key(name)
    }
    pub fn named(&self, name: &str) -> impl Iterator<Item = Row<'_>> {
        self.by_name
            .get(name)
            .into_iter()
            .flatten()
            .map(|&i| Row(&self.rows[i]))
    }
    pub fn first(&self, name: &str) -> Option<Row<'_>> {
        self.by_name.get(name)?.first().map(|&i| Row(&self.rows[i]))
    }
    pub fn last(&self, name: &str) -> Option<Row<'_>> {
        self.by_name.get(name)?.last().map(|&i| Row(&self.rows[i]))
    }
    pub fn prototype_members(&self, kinds: &[&str], key: Option<&str>) -> Vec<&'static str> {
        let mut rows = Vec::new();
        for kind in kinds {
            rows.extend(
                self.primitive_members
                    .get(*kind)
                    .into_iter()
                    .flatten()
                    .copied()
                    .filter(|&i| member_of_primitive(self.rows[i].entry.name, kind, key)),
            );
        }
        rows.sort_unstable();
        rows.dedup();
        rows.into_iter().map(|i| self.rows[i].entry.name).collect()
    }
    pub fn module_rows(&self, name: &str) -> impl Iterator<Item = Row<'_>> {
        self.by_module
            .get(name)
            .into_iter()
            .flatten()
            .map(|&i| Row(&self.rows[i]))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prototype_birth_is_not_structural_without_a_structural_edge() {
        let table = [
            RealmEntry {
                name: "Holder.prototype",
                kind: RealmKind::Prototype,
                births: &["Ctor", "Parent", "Captured", "%Layout:VALUE%"],
                module: None,
                facts: r#"{"captures":[{"kind":"reference","value":"Captured"}]}"#,
                effects: &[],
                own: &[],
                proto: Some("Parent"),
            },
            RealmEntry {
                name: "%Layout:VALUE%",
                kind: RealmKind::Exotic,
                births: &[],
                module: None,
                effects: &[],
                own: &[],
                proto: None,
                facts: r#"{"storage":{"kind":"runtime_layout","owners":["Holder.prototype"]}}"#,
            },
            RealmEntry {
                name: "%Layout:BODY%",
                kind: RealmKind::Exotic,
                births: &[],
                module: None,
                effects: &[],
                own: &[],
                proto: None,
                facts: r#"{"storage":{"kind":"runtime_layout","owners":["Other"]}}"#,
            },
        ];
        let index = RealmIndex::new(&table);
        let row = index.first("Holder.prototype").unwrap();
        assert!(
            !row.structure_has("Ctor"),
            "a prototype's birth is not a structural link"
        );
        assert!(!row.structure_has("%Layout:BODY%"));
        for name in ["Parent", "Captured", "%Layout:VALUE%"] {
            assert!(row.structure_has(name), "{name}");
        }
    }

    #[test]
    fn parses_once_per_caller_owned_table() {
        let table = [RealmEntry {
            name: "X",
            kind: crate::realm_manifest::RealmKind::Constructor,
            births: &[],
            module: None,
            facts: "{}",
            effects: &[],
            own: &[crate::realm_manifest::RealmOwn {
                key: "self",
                symbol: false,
                value: RealmOwnValue::Reference("X"),
            }],
            proto: None,
        }];
        FACTS_PARSES.with(|n| n.set(0));
        INDEX_BUILDS.with(|n| n.set(0));
        let index = RealmIndex::new(&table);
        FACTS_PARSES.with(|n| assert_eq!(n.get(), 1));
        FACTS_PARSES.with(|n| n.set(0));
        for _ in 0..3 {
            let mut reach = crate::realm_reach::RealmReach::default();
            reach.names.insert("X".into());
            reach.close(&index);
            assert_eq!(
                index.first("X").unwrap().members().collect::<Vec<_>>(),
                vec![("self", false, "X")]
            );
            let module = perry_parser::parse_typescript("X;", "index.cjs").unwrap();
            crate::realm_reach::scan(&module, &index);
        }
        FACTS_PARSES.with(|n| assert_eq!(n.get(), 0, "lookups must not parse facts"));
        INDEX_BUILDS.with(|n| assert_eq!(n.get(), 1));
    }
}

pub const PRIMITIVE_KINDS: [&str; 5] = ["String", "Number", "Boolean", "BigInt", "Symbol"];
pub fn member_of_primitive(name: &str, kind: &str, key: Option<&str>) -> bool {
    let Some(rest) = name
        .strip_prefix(kind)
        .and_then(|r| r.strip_prefix(".prototype"))
    else {
        return false;
    };
    let Some(key) = key else {
        return rest.starts_with('.') || rest.starts_with('[');
    };
    if let Some(rest) = rest.strip_prefix('.') {
        rest.strip_prefix(key)
            .is_some_and(|r| r.is_empty() || r.starts_with(".<"))
    } else {
        rest.strip_prefix("[\"")
            .and_then(|r| r.strip_prefix(key))
            .and_then(|r| r.strip_prefix("\"]"))
            .is_some_and(|r| r.is_empty() || r.starts_with(".<"))
    }
}
