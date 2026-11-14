//! Compiler-only ownership of materialize groups. Roots are declarations;
//! members inherit through the source's own-value edges. No map is emitted.
use perry_hir::realm_manifest::{RealmDeclaration, RealmValue};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct Families {
    owners: BTreeMap<String, u8>,
    roots: BTreeMap<String, u8>,
    serials: BTreeMap<String, u64>,
}

impl Families {
    pub(crate) fn empty() -> Self {
        Self {
            owners: BTreeMap::new(),
            roots: BTreeMap::new(),
            serials: BTreeMap::new(),
        }
    }

    pub(crate) fn from_declarations(
        declarations: &BTreeMap<String, RealmDeclaration>,
    ) -> Result<Self, String> {
        let mut result = Self {
            owners: BTreeMap::new(),
            roots: BTreeMap::from([("%Global%".into(), 0)]),
            serials: BTreeMap::new(),
        };
        for (name, declaration) in declarations {
            if let Some(family) = declaration.materialize_family {
                if let Some(previous) = result.roots.insert(name.clone(), family) {
                    if previous != family {
                        return Err(format!("conflicting materialize facts on {name}"));
                    }
                }
            }
            if let Some(serial) = declaration.intrinsic_serial {
                result.serials.insert(name.clone(), serial);
            }
            if let Some(constructor) = &declaration.constructor {
                result.roots.insert(name.clone(), constructor.identity);
                let prototype = declaration
                    .properties
                    .iter()
                    .find_map(|p| {
                        if !p.symbol && p.key == "prototype" {
                            match &p.value {
                                Some(RealmValue::Reference(name)) => Some(name.clone()),
                                _ => None,
                            }
                        } else {
                            None
                        }
                    })
                    .ok_or_else(|| format!("constructor {name} lacks its prototype fact"))?;
                // A source-declared shared root owns its group independently
                // of constructors whose .prototype refers to that same cell.
                // Otherwise the constructor's own prototype is its family.
                if declarations
                    .get(&prototype)
                    .and_then(|row| row.materialize_family)
                    .is_none()
                {
                    if let Some(previous) =
                        result.roots.insert(prototype.clone(), constructor.identity)
                    {
                        if previous != constructor.identity {
                            return Err(format!(
                                "conflicting prototype materialize facts on {prototype}"
                            ));
                        }
                    }
                }
                if let Some(serial) = constructor
                    .prototype_serial_value
                    .or(constructor.sealed_serial_value)
                    .or(constructor.named_serial_value)
                {
                    result.serials.insert(prototype, serial);
                }
            }
        }
        for (name, row) in declarations {
            if row.native_namespace.is_some() {
                result.roots.entry(name.clone()).or_insert(0);
            }
        }
        // A declared global value has its own Global-owned birth. Aliases
        // from a constructor (Number.parseInt, for example) do not transfer
        // that function into the constructor's materialize group.
        let global = declarations
            .get("%Global%")
            .ok_or("missing Global declaration")?;
        for property in &global.properties {
            let mut targets = Vec::new();
            targets.extend(property.get.iter().chain(&property.set).cloned());
            if let Some(value) = &property.value {
                add_value(value, &mut targets);
            }
            for name in targets {
                result.roots.entry(name).or_insert(0);
            }
        }
        // An alias does not transfer ownership of an existing cell. The
        // actual source birth can declare its owner independently of every
        // holder whose slot merely references it.
        for (name, row) in declarations {
            if let Some(owner) = &row.owner {
                let family = *result.roots.get(owner).ok_or_else(|| {
                    format!("birth owner {owner} of {name} has no materialize root")
                })?;
                if let Some(previous) = result.roots.insert(name.clone(), family) {
                    if previous != family {
                        return Err(format!("conflicting source birth owner of {name}"));
                    }
                }
            }
        }
        // A namespace reached through Global inherits Global's group. A native
        // provider has no image in S3/S4 and is handled by its declared owner.
        let roots = result.roots.clone();
        for (root, family) in roots {
            let mut todo = vec![root];
            let mut seen = BTreeSet::new();
            while let Some(name) = todo.pop() {
                if !seen.insert(name.clone()) {
                    continue;
                }
                if result
                    .roots
                    .get(&name)
                    .is_some_and(|other| *other != family)
                {
                    continue;
                }
                let Some(declaration) = declarations.get(&name) else {
                    return Err(format!(
                        "materialize root references absent declaration {name}"
                    ));
                };
                if let Some(previous) = result.owners.insert(name.clone(), family) {
                    if previous != family {
                        return Err(format!(
                            "ambiguous materialize owner of {name}: {previous}/{family}"
                        ));
                    }
                }
                // [[Prototype]] edges cross families and never confer ownership.
                for property in &declaration.properties {
                    todo.extend(property.get.iter().chain(&property.set).cloned());
                    if let Some(value) = &property.value {
                        add_value(value, &mut todo);
                    }
                }
                for value in declaration
                    .captures
                    .iter()
                    .chain(declaration.storage.iter().flat_map(|s| s.values()))
                {
                    add_value(value, &mut todo);
                }
            }
        }
        Ok(result)
    }

    pub(crate) fn family(&self, name: &str) -> Result<u8, String> {
        self.owners
            .get(name)
            .copied()
            .ok_or_else(|| format!("realm cell {name} has no source-owned materialize group"))
    }

    pub(crate) fn serial(&self, name: &str) -> Option<u64> {
        self.serials.get(name).copied()
    }
}

fn add_value(value: &RealmValue, todo: &mut Vec<String>) {
    if let RealmValue::Reference(name) | RealmValue::RawReference(name) = value {
        todo.push(name.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn declarations(text: &str) -> BTreeMap<String, RealmDeclaration> {
        let rows: Vec<RealmDeclaration> = serde_json::from_str(text).unwrap();
        rows.into_iter()
            .map(|row| (row.name.clone(), row))
            .collect()
    }

    #[test]
    fn realm_materialize_ownership_stops_at_other_roots_and_prototype_edges() {
        let rows = declarations(
            r#"[
          {"name":"%Global%","kind":"namespace","properties":[
            {"key":"Ctor","symbol":false,"attributes":7,"value":{"kind":"reference","value":"Ctor"}},
            {"key":"fn","symbol":false,"attributes":7,"value":{"kind":"reference","value":"fn"}}]},
          {"name":"Ctor","kind":"constructor","constructor":{"identity":7,"prototype_callable":false},"properties":[
            {"key":"prototype","symbol":false,"attributes":0,"value":{"kind":"reference","value":"Proto"}},
            {"key":"f","symbol":false,"attributes":7,"value":{"kind":"reference","value":"member"}},
            {"key":"alias","symbol":false,"attributes":7,"value":{"kind":"reference","value":"fn"}}]},
          {"name":"Proto","kind":"prototype","prototype":"Other","properties":[
            {"key":"f","symbol":false,"attributes":7,"value":{"kind":"reference","value":"member"}}]},
          {"name":"member","kind":"method","prototype":"Other"},
          {"name":"Other","kind":"prototype","materialize_family":128},
          {"name":"fn","kind":"function","prototype":"Other"},
          {"name":"unowned","kind":"function"}
        ]"#,
        );
        let families = Families::from_declarations(&rows).unwrap();
        assert_eq!(families.family("%Global%").unwrap(), 0);
        assert_eq!(families.family("fn").unwrap(), 0);
        assert_eq!(families.family("Ctor").unwrap(), 7);
        assert_eq!(families.family("Proto").unwrap(), 7);
        assert_eq!(families.family("member").unwrap(), 7);
        assert_eq!(families.family("Other").unwrap(), 128);
        assert!(families.family("unowned").is_err());
    }

    #[test]
    fn realm_materialize_ambiguous_own_value_is_a_source_error() {
        let rows = declarations(
            r#"[
          {"name":"%Global%","kind":"namespace"},
          {"name":"Other","kind":"prototype","materialize_family":128,"properties":[
            {"key":"f","symbol":false,"attributes":7,"value":{"kind":"reference","value":"member"}}]},
          {"name":"Third","kind":"prototype","materialize_family":129,"properties":[
            {"key":"f","symbol":false,"attributes":7,"value":{"kind":"reference","value":"member"}}]},
          {"name":"member","kind":"method"}
        ]"#,
        );
        assert!(Families::from_declarations(&rows)
            .err()
            .unwrap()
            .contains("ambiguous"));
        let mut rows = rows;
        rows.get_mut("member").unwrap().owner = Some("Other".into());
        let families = Families::from_declarations(&rows).unwrap();
        assert_eq!(families.family("member").unwrap(), 128);
        assert_eq!(families.family("Third").unwrap(), 129);
    }
}
