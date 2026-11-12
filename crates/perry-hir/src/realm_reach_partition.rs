//! The shared logical ownership and birth predicates.
use super::{is_global, primitive_prototype_member};
use crate::realm_reach_index::{RealmIndex, PRIMITIVE_KINDS};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RealmTop {
    DynamicCode,
    GlobalObjectValue,
    With,
    DynamicModule,
    GlobalAnyMember,
}

impl RealmTop {
    pub fn reason(&self) -> &'static str {
        match self {
            Self::DynamicCode => "DynamicCodeReach.globals",
            Self::GlobalObjectValue => "global object used as a value",
            Self::With => "with",
            Self::DynamicModule => "non-constant require/import",
            Self::GlobalAnyMember => "PatchedBuiltins global ANY_MEMBER",
        }
    }
}

/// Which own members a keyless own read sees: the enumerable ones
/// (`Object.keys/entries/values`, `JSON.stringify`, `for-in`, object spread
/// and rest, `Object.assign` sources) or all of them (`Reflect.ownKeys`,
/// `Object.getOwnPropertyNames/Symbols/Descriptors`, a formatted value).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum KeylessRead {
    Enumerable,
    All,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct RealmReach {
    #[serde(skip)]
    pub names: BTreeSet<String>,
    /// Reached values whose body births are live, recomputed by `close`.
    #[serde(skip)]
    pub whole: BTreeSet<String>,
    /// Held structure whose body births are not live, recomputed by `close`.
    #[serde(skip)]
    pub structure: BTreeSet<String>,
    #[serde(skip)]
    pub top: BTreeSet<RealmTop>,
    /// Prototypes supplied by program syntax, without naming constructors.
    pub chain_roots: BTreeSet<String>,
    /// A declared body may inspect a nonprimitive operand through its chain.
    pub chain_read: bool,
    /// Runtime calls emitted independently of program naming.
    pub exec: BTreeSet<String>,
    /// Global keys awaiting resolution against writes of executing operations.
    pub global_keys: BTreeSet<String>,
    /// The names the program itself names, before the birth closure (and
    /// before a top scan closes R to the whole catalog). Every such name
    /// enters through [`own`](Self::own); [`close`](Self::close) adds to
    /// `names` only.
    pub own_names: BTreeSet<String>,
    /// Static keys of member operations (and object-pattern keys) on
    /// receivers the scan cannot name: such a key owns that member of every
    /// held holder (§7 receiver rule; `fh.readableWebStream()`).
    pub receiver_keys: BTreeSet<String>,
    /// A key the scan cannot name on such a receiver: every member of every
    /// held holder is owned.
    pub any_receiver_key: bool,
    /// D3 S2: keys (None: an unknown key) read on a proven receiver (a
    /// literal or a program-class instance) whose candidates were its own
    /// prototype chain only. That proof assumes no prototype ever changes;
    /// [`close`](Self::close) applies the receiver rule to them when the
    /// program may change one (`may_set_prototype`).
    pub chain_keys: BTreeSet<Option<String>>,
    /// R1k: keys (None: an unknown key) an own-property callable
    /// (`defineProperty`, `getOwnPropertyDescriptor`, ...) called in place
    /// reads or writes on a target the scan cannot name. Such a call touches
    /// only the target's OWN property: a primitive prototype's member only
    /// when the target may be that prototype, which a program obtains as a
    /// value without naming it only by reading a prototype
    /// ([`may_read_prototype`](Self::may_read_prototype)); [`close`](Self::close)
    /// owns member K of the primitive prototypes then.
    pub own_keys: BTreeSet<Option<String>>,
    /// Ruling A (R1d): the widest keyless own read on a value the scan
    /// cannot name (a reflective or key-enumerating read). It may read any
    /// held holder, so it reads every member that holder keeps as a value
    /// (its key and the member's own cell), never the member's body births:
    /// [`keeps_member`](Self::keeps_member).
    pub keyless_read: Option<KeylessRead>,
    /// D3 S3: static keys of plain writes (`o.k = v`, `o["k"] = v`) on
    /// receivers the scan cannot name: [[Set]] runs only a setter, so such a
    /// key owns that member's `.<set>` on every held holder.
    pub setter_keys: BTreeSet<String>,
    /// A plain write with a key the scan cannot name: every setter of every
    /// held holder is owned.
    pub any_setter_key: bool,
    /// What the CommonJS wrapper's escape arms reach: the `A` of every
    /// `if (__perry_cjs_open()) A else B`. The program's closed-world bit
    /// (`cjs_closed`) deletes `A` from a closed program before codegen
    /// (`cjs_link::resolve_open_sites`), so its part is R only in an open one:
    /// [`resolve_cjs_open`](Self::resolve_cjs_open) applies that bit. A part
    /// never resolved is R ([`close`](Self::close), [`is_full`](Self::is_full)).
    pub cjs_open_arm: Option<Box<RealmReach>>,
}

impl RealmReach {
    pub fn is_full(&self) -> bool {
        !self.top.is_empty() || self.cjs_open_arm.as_ref().is_some_and(|arm| arm.is_full())
    }

    /// Apply the program's CommonJS closed-world bit to the wrapper's escape
    /// arms: an open program keeps them (their part joins R), a closed one has
    /// none (`cjs_link::resolve_open_sites` deleted them).
    pub fn resolve_cjs_open(&mut self, closed: bool) {
        if let Some(arm) = self.cjs_open_arm.take() {
            if !closed {
                self.merge(&arm);
            }
        }
    }

    /// The program names `name`: it is in R and among the program's own names.
    pub fn own(&mut self, name: &str) {
        self.names.insert(name.into());
        self.own_names.insert(name.into());
    }

    pub fn merge(&mut self, other: &Self) {
        self.names.extend(other.names.iter().cloned());
        self.own_names.extend(other.own_names.iter().cloned());
        self.receiver_keys
            .extend(other.receiver_keys.iter().cloned());
        self.any_receiver_key |= other.any_receiver_key;
        self.chain_roots.extend(other.chain_roots.iter().cloned());
        self.chain_read |= other.chain_read;
        self.exec.extend(other.exec.iter().cloned());
        self.global_keys.extend(other.global_keys.iter().cloned());
        self.chain_keys.extend(other.chain_keys.iter().cloned());
        self.own_keys.extend(other.own_keys.iter().cloned());
        self.keyless_read = self.keyless_read.max(other.keyless_read);
        self.setter_keys.extend(other.setter_keys.iter().cloned());
        self.any_setter_key |= other.any_setter_key;
        self.top.extend(other.top.iter().cloned());
        if let Some(arm) = &other.cjs_open_arm {
            self.cjs_open_arm
                .get_or_insert_with(Default::default)
                .merge(arm);
        }
    }

    /// Apply the exact program-level signals, including patches discovered in
    /// modules other than the one currently being scanned.
    pub fn program_signals(&mut self, patched: &crate::PatchedBuiltins) {
        if patched.dynamic_code_reach.globals {
            self.top.insert(RealmTop::DynamicCode);
        }
        if patched
            .iter()
            .any(|(owner, key)| is_global(owner) && key == crate::patched_builtins::ANY_MEMBER)
        {
            self.top.insert(RealmTop::GlobalAnyMember);
        }
    }

    /// Replace all program facts; a dumper retains only calls it actually emits.
    pub fn from_snapshot(snapshot: Self, exec: BTreeSet<String>, index: &RealmIndex) -> Self {
        let mut reach = snapshot;
        reach.top.clear();
        reach.names.clear();
        reach.whole.clear();
        reach.structure.clear();
        reach.exec.extend(exec);
        reach.close(index);
        reach
    }

    pub fn exec(&mut self, symbol: &str) {
        self.exec.insert(symbol.into());
    }

    pub fn executing(&self, index: &RealmIndex) -> BTreeSet<String> {
        let mut calls = self.exec.clone();
        calls.extend(
            self.whole
                .iter()
                .filter_map(|n| index.last(n))
                .filter_map(|row| row.body())
                .map(|body| body.rsplit("::").next().unwrap())
                .filter(|body| index.operation(body).is_some())
                .map(str::to_owned),
        );
        calls.extend(
            self.whole
                .iter()
                .filter_map(|n| index.last(n))
                .flat_map(|row| row.calls())
                .map(str::to_owned),
        );
        loop {
            let before = calls.len();
            for symbol in calls.clone() {
                for op in index.bodies(&symbol) {
                    calls.extend(
                        op.calls()
                            .map(|call| call.rsplit("::").next().unwrap().to_owned()),
                    );
                }
            }
            if calls.len() == before {
                return calls;
            }
        }
    }

    /// A declared slot or a write by an operation which the program executes.
    pub fn resolve<'a>(&self, index: &'a RealmIndex, holder: &str, key: &str) -> Option<&'a str> {
        index
            .last(holder)
            .and_then(|row| row.members().find(|(k, _, _)| *k == key).map(|(_, _, v)| v))
            .or_else(|| {
                index.rows().find_map(|op| {
                    let value = op.writes().find(|(h, k, _)| *h == holder && *k == key)?.2;
                    self.executes(index, op).then_some(value)
                })
            })
    }

    pub fn global_read(&mut self, index: &RealmIndex, key: &str) {
        self.exec("js_get_global_this");
        self.global_keys.insert(key.into());
        if let Some(value) = self.resolve(index, "%Global%", key) {
            self.own(value);
        }
    }

    pub fn close(&mut self, index: &RealmIndex) {
        self.resolve_cjs_open(false);
        self.whole.clear();
        self.structure.clear();
        if self.is_full() {
            self.names
                .extend(index.rows().map(|row| row.name().to_owned()));
            self.whole.clone_from(&self.names);
            return;
        }
        if !self.chain_keys.is_empty() && self.may_set_prototype() {
            for key in std::mem::take(&mut self.chain_keys) {
                for kind in PRIMITIVE_KINDS {
                    if index.has(kind) {
                        self.own(kind);
                    }
                }
                for name in index.prototype_members(&PRIMITIVE_KINDS, key.as_deref()) {
                    self.own(name);
                }
                match key {
                    Some(key) => {
                        self.receiver_keys.insert(key);
                    }
                    None => self.any_receiver_key = true,
                }
            }
        }
        if !self.own_keys.is_empty() && self.may_read_prototype() {
            for key in self.own_keys.clone() {
                for kind in PRIMITIVE_KINDS {
                    if index.has(kind) {
                        self.own(kind);
                    }
                }
                for name in index.prototype_members(&PRIMITIVE_KINDS, key.as_deref()) {
                    self.own(name);
                }
            }
        }
        self.names.extend(self.own_names.iter().cloned());
        self.names.extend(self.chain_roots.iter().cloned());
        // Starting at own_names makes WHOLE the least fixpoint of the same
        // monotone predicate used for the final partition.
        self.whole.extend(self.own_names.iter().cloned());
        loop {
            let before = (
                self.names.clone(),
                self.whole.clone(),
                self.chain_roots.clone(),
            );
            for key in self.global_keys.clone() {
                if let Some(value) = self.resolve(index, "%Global%", &key) {
                    self.own(value);
                }
            }
            // Exec births hold identities and kept slots; only naming owns bodies.
            let executing = self.executing(index);
            for symbol in &executing {
                for op in index.bodies(symbol) {
                    self.names.extend(op.demands().map(str::to_owned));
                    for birth in op
                        .births()
                        .filter(|b| !op.members().any(|(_, _, v)| v == *b))
                    {
                        if let Some(proto) = index.last(birth).and_then(|r| r.instance_prototype())
                        {
                            self.chain_roots.insert(proto.into());
                        } else if self.follows_birth_with(index, op, birth, &executing) {
                            self.names.insert(birth.into());
                        }
                    }
                    for (_, _, value) in op.writes() {
                        self.names.insert(value.into());
                    }
                }
            }
            let (value_born, chain_born) = self.birth_roles(index);
            self.names.extend(value_born.iter().cloned());
            self.names.extend(chain_born.iter().cloned());
            for name in self.names.clone() {
                if self.is_whole(&name, index, &value_born, &chain_born) {
                    self.whole.insert(name);
                }
            }
            self.structure = self.names.difference(&self.whole).cloned().collect();
            for name in self.names.clone() {
                let Some(row) = index.last(&name) else {
                    continue;
                };
                for birth in row.births() {
                    if !row.members().any(|(_, _, v)| v == birth)
                        && self.whole.contains(&name)
                        && index.kind_birth(birth)
                    {
                        if let Some(proto) = index.last(birth).and_then(|b| b.instance_prototype())
                        {
                            self.chain_roots.insert(proto.to_owned());
                        }
                    }
                    if self.follows_birth_with(index, row, birth, &executing) {
                        self.names.insert(birth.to_owned());
                    }
                }
            }
            if before
                == (
                    self.names.clone(),
                    self.whole.clone(),
                    self.chain_roots.clone(),
                )
            {
                break;
            }
        }
        self.structure = self.names.difference(&self.whole).cloned().collect();
    }

    fn birth_roles(&self, index: &RealmIndex) -> (BTreeSet<String>, BTreeSet<String>) {
        let mut values = BTreeSet::new();
        let mut chains = self.chain_roots.clone();
        for name in self.whole.iter().map(String::as_str) {
            let Some(row) = index.last(name) else {
                continue;
            };
            for birth in row.births() {
                if Some(birth) != row.prototype() && !row.members().any(|(_, _, v)| v == birth) {
                    if index.kind_birth(birth) {
                        if let Some(proto) = index.last(birth).and_then(|b| b.instance_prototype())
                        {
                            chains.insert(proto.to_owned());
                        }
                    } else {
                        values.insert(birth.to_owned());
                    }
                }
            }
            if let Some(proto) = row.prototype() {
                chains.insert(proto.to_owned());
            }
        }
        if let Some(proto) = index.last("%Global%").and_then(|r| r.prototype()) {
            chains.insert(proto.into());
        }
        let mut pending: Vec<_> = chains.iter().cloned().collect();
        while let Some(name) = pending.pop() {
            if let Some(proto) = index.last(&name).and_then(|r| r.prototype()) {
                if chains.insert(proto.to_owned()) {
                    pending.push(proto.to_owned());
                }
            }
        }
        (values, chains)
    }

    fn is_whole(
        &self,
        name: &str,
        index: &RealmIndex,
        value_born: &BTreeSet<String>,
        chain_born: &BTreeSet<String>,
    ) -> bool {
        self.own_names.contains(name)
            || self.owned_member(index, name)
            || value_born.contains(name)
            || (chain_born.contains(name) && (self.chain_read || self.may_read_prototype()))
    }

    #[cfg(test)]
    pub(super) fn assert_whole_fixpoint(&self, index: &RealmIndex) {
        let (values, chains) = self.birth_roles(index);
        for name in &self.names {
            assert_eq!(
                self.whole.contains(name),
                self.is_whole(name, index, &values, &chains),
                "WHOLE fixpoint: {name}"
            );
        }
    }

    pub fn executes(&self, index: &RealmIndex, row: crate::realm_reach_index::Row<'_>) -> bool {
        self.whole.contains(row.name())
            || row.operation().or_else(|| row.body()).is_some_and(|body| {
                self.executing(index)
                    .contains(body.rsplit("::").next().unwrap())
            })
    }

    /// Logical birth closure and physical member selection share this rule.
    pub fn follows_birth(
        &self,
        index: &RealmIndex,
        row: crate::realm_reach_index::Row<'_>,
        birth: &str,
    ) -> bool {
        self.follows_birth_with(index, row, birth, &self.executing(index))
    }

    fn follows_birth_with(
        &self,
        index: &RealmIndex,
        row: crate::realm_reach_index::Row<'_>,
        birth: &str,
        executing: &BTreeSet<String>,
    ) -> bool {
        let mut member = false;
        for (key, symbol, value) in row.members().filter(|(_, _, value)| *value == birth) {
            member = true;
            if self.keeps_member(index, row, key, value, row.member_enumerable(key, symbol)) {
                return true;
            }
        }
        if member {
            return false;
        }
        ((self.whole.contains(row.name())
            || row
                .operation()
                .or_else(|| row.body())
                .is_some_and(|body| executing.contains(body.rsplit("::").next().unwrap())))
            && !index.kind_birth(birth))
            || (self.names.contains(row.name())
                && row.structure_has(birth)
                && !(birth.starts_with("%Layout:") && primitive_prototype_member(row.name())))
    }

    /// Keeping a slot needs its value's identity, not its body's births.
    /// Bind reads constructor.prototype by key without owning the prototype.
    /// A physical global cell alone does not hold every global as a value;
    /// keyless retention requires the holder itself in the program's R.
    pub fn keeps_member(
        &self,
        index: &RealmIndex,
        holder: crate::realm_reach_index::Row<'_>,
        key: &str,
        value: &str,
        enumerable: bool,
    ) -> bool {
        self.owned_member(index, value)
            || index.rows().any(|op| {
                op.writes()
                    .any(|(h, k, v)| h == holder.name() && k == key && v == value)
                    && self.resolve(index, holder.name(), key) == Some(value)
                    && self.executes(index, op)
            })
            || (self.names.contains(holder.name())
                && ((key == "prototype" && holder.has_constructor())
                    || self.own_keys.contains(&None)
                    || self.keyless_read == Some(KeylessRead::All)
                    || (enumerable && self.keyless_read == Some(KeylessRead::Enumerable))))
    }

    /// The program may change an object's prototype: it owns a member that
    /// sets one (`setPrototypeOf`, the `__proto__` setter), reads or writes
    /// the static key `setPrototypeOf`/`__proto__` on a receiver the scan
    /// cannot name, or reads an unknown key on one (which may yield
    /// `Object.setPrototypeOf`). An unknown computed key is
    /// never `"__proto__"`.
    fn may_set_prototype(&self) -> bool {
        self.any_receiver_key
            || ["setPrototypeOf", "__proto__"]
                .iter()
                .any(|key| self.receiver_keys.contains(*key))
            || self.setter_keys.contains("__proto__")
            || self.own_names.iter().any(|name| {
                name.ends_with(".setPrototypeOf")
                    || (name.contains("__proto__") && name.ends_with(".<set>"))
            })
    }

    /// A [[Prototype]] observation exposes chain cells as values. Reading a
    /// slot called `prototype` only keeps that slot; it observes no members
    /// of the value stored there (OrdinaryHasInstance compares its identity).
    fn may_read_prototype(&self) -> bool {
        self.any_receiver_key
            || [
                "__proto__",
                "getPrototypeOf",
                "isPrototypeOf",
                "setPrototypeOf",
            ]
            .iter()
            .any(|key| self.receiver_keys.contains(*key))
            || self.own_names.iter().any(|name| {
                name.ends_with(".getPrototypeOf")
                    || name.ends_with(".isPrototypeOf")
                    || name.ends_with(".setPrototypeOf")
                    || name.contains("__proto__")
            })
    }

    /// Ownership is a naming/read event on a member of a held holder.
    fn owned_member(&self, index: &RealmIndex, name: &str) -> bool {
        if self.own_names.contains(name) {
            return true;
        }
        let setter = name.ends_with(".<set>");
        self.names
            .iter()
            .filter_map(|n| index.last(n))
            .any(|holder| {
                holder.members().any(|(key, symbol, value)| {
                    value == name
                        // A prototype slot read observes the cell's identity,
                        // not the members/body of the value stored there.
                        && key != "prototype"
                        && (self.any_receiver_key
                            || (setter && self.any_setter_key)
                            || holder.runtime_layout_holder()
                            || (self.whole.contains(holder.name())
                                && (symbol
                                    || key.contains("@@")
                                    || holder.runtime_spelled_key(key)))
                            || self.receiver_keys.contains(key)
                            || (setter && self.setter_keys.contains(key)))
                })
            })
    }
}
