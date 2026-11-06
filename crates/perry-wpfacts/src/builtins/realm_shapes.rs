//! The declared table as object shapes, built once for a model. References
//! name the same objects as the source table; tokens retain their shape.

use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Default)]
pub(crate) struct Property {
    pub key: &'static str,
    pub native: Option<&'static str>,
    pub getter_this: bool,
    pub own_namespace: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct Coordinates {
    pub prototype: Option<(Family, Option<&'static str>, &'static str)>,
    pub namespace: Option<(BuiltinNs, &'static str)>,
    pub constructible: bool,
}

pub(crate) enum GlobalIdentity {
    None,
    One(&'static str),
    Many,
}

impl GlobalIdentity {
    pub fn matches(&self, identity: &str) -> bool {
        match self {
            Self::None => true,
            Self::One(value) => *value == identity,
            Self::Many => false,
        }
    }
}

pub(crate) struct Shape {
    pub name: &'static str,
    pub properties: Vec<Property>,
    pub native: Option<Coordinates>,
    pub namespace: Option<BuiltinNs>,
    pub family: Option<(Family, Option<&'static str>)>,
    pub prototype: Option<usize>,
    pub global_identity: GlobalIdentity,
}

pub(crate) struct Declarations {
    shapes: Vec<Shape>,
    named: usize,
}

fn coordinates(owner: &'static str, key: &'static str) -> Coordinates {
    Coordinates {
        prototype: super::prototype_model(owner, key),
        namespace: (!owner.ends_with(".prototype"))
            .then(|| ns_by_name(owner))
            .flatten()
            .map(|ns| {
                (
                    if ns == BuiltinNs::GlobalThis {
                        BuiltinNs::Other
                    } else {
                        ns
                    },
                    key,
                )
            }),
        constructible: false,
    }
}

impl Default for Declarations {
    fn default() -> Self {
        let mut holders: BTreeMap<&'static str, BTreeMap<&'static str, Property>> = BTreeMap::new();
        let mut namespaces: BTreeMap<BuiltinNs, (&'static str, BTreeMap<&'static str, Property>)> =
            BTreeMap::new();
        let mut identities = BTreeSet::new();
        let mut canonical = BTreeMap::new();
        let mut global_values: BTreeMap<BuiltinNs, BTreeSet<&'static str>> = BTreeMap::new();
        for line in super::realm_declared::DECLARED
            .lines()
            .filter(|s| !s.starts_with('#'))
        {
            let mut row = line.split('\t');
            let (Some(kind), Some(owner), Some(key)) = (row.next(), row.next(), row.next()) else {
                continue;
            };
            let value = row.next();
            let property = holders
                .entry(owner)
                .or_default()
                .entry(key)
                .or_insert(Property {
                    key,
                    ..Default::default()
                });
            match kind {
                "native" => {
                    let identity = value.expect("native identity");
                    property.native = Some(identity);
                    identities.insert(identity);
                    let exact = owner == "%Global%" && key == identity
                        || identity.strip_prefix(owner).is_some_and(|suffix| {
                            suffix.strip_prefix('.') == Some(key)
                                || suffix
                                    .strip_prefix("[\"")
                                    .and_then(|s| s.strip_suffix("\"]"))
                                    == Some(key)
                        });
                    if exact {
                        canonical.entry(identity).or_insert((owner, key));
                    }
                    if owner == "%Global%" {
                        if let Some(ns) = ns_by_name(identity) {
                            global_values.entry(ns).or_default().insert(identity);
                        }
                    }
                }
                "getter-this" => property.getter_this = true,
                "ns" => property.own_namespace = true,
                _ => {}
            }
            if let Some(ns) = ns_by_name(owner) {
                let entry = namespaces.entry(ns).or_insert((owner, BTreeMap::new()));
                entry.0 = entry.0.min(owner);
                let p = entry.1.entry(key).or_insert(Property {
                    key,
                    ..Default::default()
                });
                if kind == "native" && p.native.is_none() {
                    p.native = value;
                }
                if kind == "getter-this" {
                    p.getter_this = true;
                }
                if kind == "ns" {
                    p.own_namespace = true;
                }
            }
        }
        for &identity in &identities {
            holders.entry(identity).or_default();
        }
        let mut shapes: Vec<_> = holders
            .into_iter()
            .map(|(name, properties)| {
                let native = identities.contains(name).then_some(()).and_then(|()| {
                    let (owner, key) = canonical.get(name).copied().or_else(|| {
                        if let Some((owner, key)) = name.rsplit_once("[\"") {
                            Some((owner, key.strip_suffix("\"]")?))
                        } else {
                            name.rsplit_once('.')
                        }
                    })?;
                    Some(coordinates(owner, key))
                });
                Shape {
                    name,
                    properties: properties.into_values().collect(),
                    native,
                    namespace: None,
                    family: None,
                    prototype: None,
                    global_identity: GlobalIdentity::None,
                }
            })
            .collect();
        let named = shapes.len();
        for entry in crate::native_effects::realm() {
            if entry.kind == perry_hir::realm_manifest::RealmKind::Constructor {
                if let Ok(i) = shapes.binary_search_by_key(&entry.name, |s| s.name) {
                    if let Some(native) = shapes[i].native.as_mut() {
                        native.constructible = true;
                    }
                }
            }
        }
        for (ns, (name, properties)) in namespaces {
            let prototype = loads::proto_family(ns).and_then(|_| {
                let name = format!("{name}.prototype");
                shapes[..named]
                    .binary_search_by_key(&name.as_str(), |s| s.name)
                    .ok()
            });
            let family = loads::proto_family(ns).map(|(f, w, _)| (f, w));
            shapes.push(Shape {
                name,
                properties: properties.into_values().collect(),
                native: None,
                namespace: Some(ns),
                family,
                prototype,
                global_identity: match global_values.remove(&ns) {
                    Some(values) if values.len() == 1 => {
                        GlobalIdentity::One(*values.first().unwrap())
                    }
                    Some(values) if !values.is_empty() => GlobalIdentity::Many,
                    _ => GlobalIdentity::None,
                },
            });
        }
        shapes[named..].sort_by_key(|s| s.name);
        Self { shapes, named }
    }
}

impl Declarations {
    pub fn shape(&self, i: usize) -> &Shape {
        &self.shapes[i]
    }

    pub fn named(&self, name: &str, m: &Model) -> Option<usize> {
        self.shapes[..self.named]
            .binary_search_by(|s| {
                m.charge_work(1);
                s.name.cmp(name)
            })
            .ok()
    }

    pub fn namespace(&self, ns: BuiltinNs, m: &Model) -> Option<usize> {
        let ns = if ns == BuiltinNs::Other {
            BuiltinNs::GlobalThis
        } else {
            ns
        };
        self.shapes[self.named..]
            .iter()
            .position(|s| {
                m.charge_work(1);
                s.namespace == Some(ns)
            })
            .map(|i| i + self.named)
    }

    pub fn property(&self, shape: usize, key: &str, m: &Model) -> Option<Property> {
        let properties = &self.shapes[shape].properties;
        let key = key.replace(WK_PREFIX, "@@");
        properties
            .binary_search_by(|p| {
                m.charge_work(1);
                p.key.cmp(&key)
            })
            .ok()
            .map(|i| properties[i])
    }

    pub fn family(&self, family: Family, m: &Model) -> Option<usize> {
        self.shapes[self.named..].iter().find_map(|s| {
            m.charge_work(1);
            (s.family == Some((family, None)))
                .then_some(s.prototype)
                .flatten()
        })
    }

    pub fn resolve(&self, kind: &TokKind, m: &Model) -> Option<usize> {
        match kind {
            TokKind::Builtin(ns) => {
                if let BuiltinNs::ObjectProtoOwn(own) = ns {
                    self.shapes[..self.named].iter().position(|s| {
                        m.charge_work(1);
                        s.name
                            .strip_prefix("Object.prototype.")
                            .and_then(ProtoOwn::of_name)
                            == Some(*own)
                    })
                } else {
                    self.namespace(*ns, m)
                }
            }
            TokKind::BuiltinFn(Native::Member { identity }) => self.named(identity, m),
            _ => None,
        }
    }
}

impl Model {
    pub(crate) fn native_shape(&self, t: Tok) -> Option<&Shape> {
        Some(self.declarations.shape(self.toks.shape(t)?))
    }

    pub(crate) fn native_coordinates(&self, t: Tok) -> Option<Coordinates> {
        self.native_shape(t)?.native
    }

    pub(crate) fn native_property(&self, t: Tok, key: &str) -> Option<Property> {
        self.declarations.property(self.toks.shape(t)?, key, self)
    }

    pub(crate) fn namespace_property(&self, ns: BuiltinNs, key: &str) -> Option<Property> {
        // Interning finds an existing value without resolving its shape again.
        // Other is the HIR global sentinel, whose shape is the global object's.
        let actual = if ns == BuiltinNs::Other {
            BuiltinNs::GlobalThis
        } else {
            ns
        };
        let t = self
            .tok_index
            .get(&TokKind::Builtin(actual))
            .copied()
            .or_else(|| {
                (actual == BuiltinNs::GlobalThis)
                    .then(|| {
                        self.tok_index
                            .get(&TokKind::Builtin(BuiltinNs::Other))
                            .copied()
                    })
                    .flatten()
            });
        if let Some(t) = t {
            return self.native_property(t, key);
        }
        let shape = self.declarations.namespace(ns, self)?;
        self.declarations.property(shape, key, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_shapes_equal_every_declared_property_and_canonical_call() {
        let m = Model::default();
        for shape in &m.declarations.shapes[..m.declarations.named] {
            for property in &shape.properties {
                assert_eq!(
                    property.native,
                    realm_declared::native_identity(shape.name, property.key)
                );
                assert_eq!(
                    property.own_namespace,
                    realm_declared::namespace_own_key(shape.name, property.key)
                );
            }
            if let Some((owner, key)) = realm_declared::identity_member(shape.name) {
                // Only callable references carry a call shape.
                if let Some(native) = shape.native {
                    assert_eq!(
                        native.prototype,
                        prototype_model(owner, key),
                        "{}",
                        shape.name
                    );
                    let ns = (!owner.ends_with(".prototype"))
                        .then(|| ns_by_name(owner))
                        .flatten()
                        .map(|ns| {
                            (
                                if ns == BuiltinNs::GlobalThis {
                                    BuiltinNs::Other
                                } else {
                                    ns
                                },
                                key,
                            )
                        });
                    assert_eq!(native.namespace, ns, "{}", shape.name);
                }
            } else {
                assert!(
                    shape.native.is_none(),
                    "{} has no canonical coordinates",
                    shape.name
                );
            }
        }
        for shape in &m.declarations.shapes[m.declarations.named..] {
            let ns = shape.namespace.unwrap();
            for key in shape
                .properties
                .iter()
                .map(|p| p.key)
                .chain(["absentCxidField"])
            {
                let property = m.namespace_property(ns, key);
                assert_eq!(
                    property.and_then(|p| p.native),
                    realm_declared::namespace_identity(ns, key),
                    "{ns:?}.{key}"
                );
                assert_eq!(
                    property.is_some_and(|p| p.getter_this),
                    realm_declared::getter_returns_receiver(ns, key),
                    "{ns:?}.{key}"
                );
            }
            for identity in [shape.name, "absentCxidNative"] {
                assert_eq!(
                    shape.global_identity.matches(identity),
                    realm_declared::namespace_singleton(ns, identity),
                    "{ns:?}/{identity}"
                );
            }
        }
    }
}
