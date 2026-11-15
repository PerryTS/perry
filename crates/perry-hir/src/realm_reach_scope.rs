//! Compile-time observations resolved by unguarded whole-program value sets.
use crate::realm_reach::{KeylessRead, RealmReach};
use crate::realm_reach_index::RealmIndex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Operand {
    /// A source operand whose value has no exported solve node.
    Unproved,
    Receiver,
    NamedReceiver(String),
    Callee,
    This,
    Arg(u32),
    Rest(u32),
    Result,
}

/// A program layer's definite own keys stop a keyed lookup before the realm.
/// A realm layer names the declaration whose prototype continues the chain.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Layer {
    pub realm: Option<String>,
    pub own: BTreeSet<String>,
    pub token: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Values {
    pub top: bool,
    pub chains: Vec<Vec<Layer>>,
    /// Boxing is used only for member lookups, never reflective/coercion reads.
    pub primitive_prototypes: BTreeSet<String>,
    pub program_bodies: bool,
    pub known_callees: bool,
    pub numeric_key: bool,
}

#[derive(Clone, Debug, Default)]
pub struct OperandIndex {
    pub complete: bool,
    pub sites: BTreeMap<(String, u32, Operand), Values>,
}
impl OperandIndex {
    pub fn get(&self, source: &str, offset: u32, operand: &Operand) -> Option<&Values> {
        self.complete.then_some(())?;
        let value = self.sites.get(&(source.into(), offset, operand.clone()))?;
        (!value.top).then_some(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Keyless(KeylessRead),
    Chain,
    Prototype(String),
    Keys {
        keys: BTreeSet<String>,
        own: bool,
    },
    Member {
        key: Option<String>,
        write: bool,
        own: bool,
    },
    UnknownCall,
    Escape {
        callback: Option<Operand>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    pub source: String,
    pub offset: u32,
    pub operand: Operand,
    pub kind: Kind,
    /// The observation's exact conservative meaning, including primitive roots.
    pub conservative: Box<RealmReach>,
}

impl RealmReach {
    pub fn resolve_observations(&mut self, facts: &OperandIndex, index: &RealmIndex) {
        while !self.observations.is_empty() {
            for observation in std::mem::take(&mut self.observations) {
                let value = facts.get(
                    &observation.source,
                    observation.offset,
                    &observation.operand,
                );
                if let Kind::Escape {
                    callback: Some(callback),
                } = &observation.kind
                {
                    if facts
                        .get(&observation.source, observation.offset, callback)
                        .is_some_and(|v| v.program_bodies)
                    {
                        continue;
                    }
                    let mut conservative = *observation.conservative;
                    conservative.resolve_observations(&OperandIndex::default(), index);
                    self.merge(&conservative);
                    continue;
                }
                let Some(value) = value else {
                    let mut conservative = *observation.conservative;
                    conservative.resolve_observations(&OperandIndex::default(), index);
                    self.merge(&conservative);
                    continue;
                };
                if observation.kind == Kind::UnknownCall {
                    if !value.known_callees {
                        let mut conservative = *observation.conservative;
                        conservative.resolve_observations(&OperandIndex::default(), index);
                        self.merge(&conservative);
                    }
                    continue;
                }
                if let Kind::Prototype(name) = &observation.kind {
                    self.scoped_prototypes.insert(name.clone());
                }
                self.apply_observation(&observation.kind, value, index);
            }
        }
        if let Some(arm) = &mut self.cjs_open_arm {
            arm.resolve_observations(facts, index);
        }
    }

    fn apply_observation(&mut self, kind: &Kind, values: &Values, index: &RealmIndex) {
        for chain in &values.chains {
            match kind {
                Kind::Keyless(read) => {
                    if let Some(holder) = chain.first().and_then(|l| l.realm.as_ref()) {
                        let current = self.keyless_on.entry(holder.clone()).or_insert(*read);
                        *current = (*current).max(*read);
                    }
                }
                Kind::Chain | Kind::Prototype(_) | Kind::Escape { .. } => {
                    self.observed_chain
                        .extend(chain.iter().filter_map(|l| l.realm.clone()));
                    if matches!(kind, Kind::Escape { .. }) {
                        self.any_key_on
                            .extend(chain.iter().filter_map(|l| l.realm.clone()));
                    }
                }
                Kind::Keys { keys, own } => {
                    for key in keys {
                        self.scoped_key(chain, Some(key), false, *own, index);
                    }
                }
                Kind::Member { key, write, own } => {
                    if key.is_some() || !values.numeric_key {
                        self.scoped_key(chain, key.as_deref(), *write, *own, index);
                    }
                }
                Kind::UnknownCall => unreachable!(),
            }
        }
        if matches!(kind, Kind::Keys { .. } | Kind::Member { .. }) {
            for prototype in &values.primitive_prototypes {
                let chain = [Layer {
                    realm: Some(prototype.clone()),
                    own: BTreeSet::new(),
                    token: None,
                }];
                match kind {
                    Kind::Keys { keys, own } => {
                        for key in keys {
                            self.scoped_key(&chain, Some(key), false, *own, index);
                        }
                    }
                    Kind::Member { key, write, own } if key.is_some() || !values.numeric_key => {
                        self.scoped_key(&chain, key.as_deref(), *write, *own, index)
                    }
                    _ => {}
                }
            }
        }
    }

    fn scoped_key(
        &mut self,
        chain: &[Layer],
        key: Option<&str>,
        write: bool,
        own: bool,
        index: &RealmIndex,
    ) {
        for (i, layer) in chain.iter().enumerate() {
            if own && i != 0 {
                break;
            }
            if let Some(holder) = &layer.realm {
                let mut next = Some(holder.as_str());
                let mut seen = BTreeSet::new();
                while let Some(holder) = next.filter(|h| seen.insert((*h).to_owned())) {
                    let Some(row) = index.last(holder) else {
                        break;
                    };
                    match key {
                        Some(key) => {
                            if row.entry().own.iter().any(|p| p.key == key) {
                                self.keys_on
                                    .entry(holder.into())
                                    .or_default()
                                    .insert((key.into(), write));
                                return;
                            }
                        }
                        None if write => {
                            self.any_setter_on.insert(holder.into());
                        }
                        None => {
                            self.any_key_on.insert(holder.into());
                        }
                    }
                    next = if own { None } else { row.prototype() };
                }
                return;
            }
            if key.is_some_and(|key| layer.own.contains(key)) {
                return;
            }
        }
    }

    pub(super) fn chain_observed(&self, name: &str, index: &RealmIndex) -> bool {
        if self.chain_read || self.may_read_prototype() {
            return true;
        }
        self.observed_chain.iter().any(|holder| {
            let mut next = Some(holder.as_str());
            let mut seen = BTreeSet::new();
            while let Some(holder) = next.filter(|h| seen.insert((*h).to_owned())) {
                if holder == name {
                    return true;
                }
                next = index.last(holder).and_then(|r| r.prototype());
            }
            false
        })
    }
}
