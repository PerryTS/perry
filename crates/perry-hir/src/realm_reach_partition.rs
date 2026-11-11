//! The shared logical ownership and birth predicates.
use super::{is_global, primitive_prototype_member};
use crate::realm_reach_index::{RealmIndex, PRIMITIVE_KINDS};
use std::collections::{BTreeMap, BTreeSet};

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
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum KeylessRead {
    Enumerable,
    All,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RealmReach {
    pub names: BTreeSet<String>,
    /// Reached values whose body births are live, recomputed by `close`.
    pub whole: BTreeSet<String>,
    /// Held structure whose body births are not live, recomputed by `close`.
    pub structure: BTreeSet<String>,
    pub top: BTreeSet<RealmTop>,
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

    /// Test-only (the realm-diff gate's `PERRY_REALM_REACH_NAMES` input):
    /// R becomes exactly `names`, with every top reason dropped, so a program
    /// that reflects over `globalThis` (the realm dumper) can be built with a
    /// partial image. A top program's own names are not kept: a scan closes a
    /// top R to the whole catalog, so they are no longer distinguishable.
    /// [`close`](Self::close) then applies the birth relation as usual.
    pub fn restrict_to(&mut self, names: impl IntoIterator<Item = String>) {
        self.top.clear();
        self.own_names = names.into_iter().collect();
        self.names.clone_from(&self.own_names);
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
        if let Some(global) = index.last("%Global%") {
            self.names.extend(
                global
                    .births()
                    .filter(|b| b.starts_with("%Layout:"))
                    .map(str::to_owned),
            );
        }
        // Membership is taken from held holders. Revisit it when a newly held
        // holder or a promoted body changes the ownership of an existing cell.
        loop {
            let before = (self.names.clone(), self.whole.clone());
            let mut members = BTreeMap::<String, Vec<_>>::new();
            for name in &self.names {
                if let Some(row) = index.last(name) {
                    for (key, symbol, value) in row.members() {
                        members.entry(value.to_owned()).or_default().push((
                            key.to_owned(),
                            symbol,
                            row,
                        ));
                    }
                }
            }
            for name in self.names.clone() {
                let owned = self.own_names.contains(&name)
                    || members.get(&name).map_or_else(
                        || !primitive_prototype_member(&name),
                        |keys| self.owned_member(&name, keys),
                    );
                if owned {
                    self.whole.insert(name.clone());
                }
                if !self.whole.contains(&name) {
                    self.structure.insert(name);
                }
            }
            for name in self.names.clone() {
                let Some(row) = index.last(&name) else {
                    continue;
                };
                for birth in row.births() {
                    if self.follows_birth(row, birth) {
                        self.names.insert(birth.to_owned());
                    }
                }
            }
            if before == (self.names.clone(), self.whole.clone()) {
                break;
            }
        }
        self.structure = self.names.difference(&self.whole).cloned().collect();
    }

    /// The one birth predicate used by the logical and physical closes.
    /// Layouts of held objects remain live even when only structure is held.
    /// The global is in neither set: its structure is its runtime layout
    /// (in every R, `close`), and a global read is a value root naming the
    /// member it reads.
    pub fn follows_birth(&self, row: crate::realm_reach_index::Row<'_>, birth: &str) -> bool {
        let name = row.name();
        self.whole.contains(name)
            || (self.structure.contains(name)
                && (row.structure_has(birth)
                    || (birth.starts_with("%Layout:") && !primitive_prototype_member(name))))
    }

    /// The one member predicate of an image-derived holder's selection: the
    /// image keeps member `value` (its descriptor `enumerable` or not) when
    /// the member is reached (its body births are live), or when it is held
    /// and a keyless own read that sees it may read its holder: an own-
    /// property callable with an unknown key (`own_keys` None) or a keyless
    /// read (`keyless_read`; an enumerable-only one sees enumerable members
    /// only). Such a read needs the member as a value, not its body.
    pub fn keeps_member(&self, value: &str, enumerable: bool) -> bool {
        self.whole.contains(value)
            || (self.names.contains(value)
                && (self.own_keys.contains(&None)
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

    /// R1k: the program may hold a primitive prototype as a value without
    /// naming it (`K.prototype` named is a holder: it escapes or is read by
    /// key): it reads a prototype through `getPrototypeOf` or `__proto__`,
    /// reads key `prototype` on a receiver the scan cannot name (`s.constructor
    /// .prototype`), or reads an unknown key on such a receiver. An
    /// own-property callable reads key `prototype` (an unknown key there
    /// includes it) only off a constructor: a primitive one the program holds
    /// unnamed only through a `constructor` read (a named one escapes, which
    /// owns its prototype's members already, `own_all`).
    fn may_read_prototype(&self) -> bool {
        self.any_receiver_key
            || ["__proto__", "prototype"]
                .iter()
                .any(|key| self.receiver_keys.contains(*key))
            || self.own_names.iter().any(|name| {
                name.ends_with(".getPrototypeOf")
                    || (name.contains("__proto__") && !name.ends_with(".<set>"))
            })
            || (self.own_keys.contains(&Some("prototype".to_string()))
                && self.may_hold_constructor())
    }

    /// R1k: the program may hold a primitive constructor as a value without
    /// naming it: it reads key `constructor` on a receiver the scan cannot
    /// name or through an own-property callable, or owns a prototype's
    /// `constructor` member (`"s".constructor`).
    fn may_hold_constructor(&self) -> bool {
        self.receiver_keys.contains("constructor")
            || self.own_keys.contains(&Some("constructor".to_string()))
            || self
                .own_names
                .iter()
                .any(|name| name.ends_with(".constructor"))
    }

    /// §7: member `name` of a held holder (named by it under `keys`) is
    /// owned: the program names it, a receiver the scan cannot name reads its
    /// key (or any key), or its key is one the runtime spells (protocol and
    /// symbol members), or its holder's layout is fixed by the runtime
    /// layout descriptor (the image cannot drop a slot of a runtime layout).
    fn owned_member(
        &self,
        name: &str,
        keys: &[(String, bool, crate::realm_reach_index::Row<'_>)],
    ) -> bool {
        let setter = name.ends_with(".<set>");
        self.own_names.contains(name)
            || self.any_receiver_key
            || (setter && self.any_setter_key)
            || keys.iter().any(|(key, symbol, holder)| {
                holder.runtime_layout_holder()
                    || (self.whole.contains(holder.name())
                        && (*symbol || key.contains("@@") || holder.runtime_spelled_key(key)))
                    // Bind reads each intrinsic constructor's prototype by key.
                    || (key == "prototype" && holder.has_constructor())
                    || self.receiver_keys.contains(key)
                    || (setter && self.setter_keys.contains(key))
            })
    }
}
