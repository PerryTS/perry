//! HOSTSURF H2: the host surfaces of one solve (design HOSTSURF.md §2.1).
//!
//! A compile that links a native provider loads its `host-surface-v1`
//! manifests into one [`HostRealm`]: the provider's declarations beside the
//! static realm table (`perry_realm_table::REALM`), in the same
//! `RealmEntry` shape, leaked once per compile. Every realm lookup the
//! solver makes by name (`builtins::members::realm_entry`, and through it
//! the chain walks and the declared natives) asks the static table first
//! and then this one; `%Global%` reads as the merged object (the static own
//! keys and the providers' globals, its [[Prototype]] the provider's
//! `realm.global` prototype when one declares it).
//!
//! The table belongs to one solve, never to the process: the facts entry
//! points install it for their duration ([`Installed`]) on the solving
//! thread (the solve runs on one thread; unit tests solve on many).
use perry_hir::realm_manifest::host_surface as hs;
use perry_hir::realm_manifest::{RealmEntry, RealmKind};
use std::cell::Cell;

/// The loaded host surfaces of one compile.
#[derive(Debug)]
pub struct HostRealm {
    pub table: hs::HostTable,
    /// The host entries, sorted by name (binary search, as the static table).
    sorted: Vec<&'static RealmEntry>,
    /// `%Global%` merged with the providers' globals.
    global: RealmEntry,
}

thread_local! {
    static HOST: Cell<Option<&'static HostRealm>> = const { Cell::new(None) };
}

/// Loads and validates `surfaces` against the static realm. `None` for no
/// surface (the static realm alone); an error names the first violation.
pub fn load(surfaces: &[hs::HostSurface]) -> Result<Option<&'static HostRealm>, String> {
    if surfaces.is_empty() {
        return Ok(None);
    }
    let realm = perry_realm_table::REALM;
    let find = |n: &str| realm.binary_search_by_key(&n, |e| e.name).ok().map(|i| &realm[i]);
    let stat = find("%Global%").ok_or("the static realm declares no %Global%")?;
    let has = |n: &str| find(n).is_some();
    let global_has = |k: &str| stat.own.iter().any(|o| !o.symbol && o.key == k);
    let table = hs::load(
        surfaces,
        &hs::StaticRealm {
            has: &has,
            global_has: &global_has,
        },
    )?;
    let mut own = stat.own.to_vec();
    own.extend(table.global_own.iter().copied());
    let global = RealmEntry {
        own: Box::leak(own.into_boxed_slice()),
        proto: table.global_proto.or(stat.proto),
        ..*stat
    };
    let h: &'static mut HostRealm = Box::leak(Box::new(HostRealm {
        table,
        sorted: Vec::new(),
        global,
    }));
    let entries: &'static [RealmEntry] = &h.table.entries;
    let mut sorted: Vec<&'static RealmEntry> = entries.iter().collect();
    sorted.sort_by_key(|e| e.name);
    h.sorted = sorted;
    Ok(Some(h))
}

impl HostRealm {
    /// The host declaration `name` (a provider-prefixed name), or the
    /// merged `%Global%`.
    pub fn entry(&'static self, name: &str) -> Option<&'static RealmEntry> {
        if name == "%Global%" {
            return Some(&self.global);
        }
        self.sorted
            .binary_search_by_key(&name, |e| e.name)
            .ok()
            .map(|i| self.sorted[i])
    }
}

/// The host realm of the solve running on this thread.
pub(crate) fn current() -> Option<&'static HostRealm> {
    HOST.with(|h| h.get())
}

/// The host declaration `name` of the running solve (`%Global%`: the
/// merged object).
pub(crate) fn entry(name: &str) -> Option<&'static RealmEntry> {
    current()?.entry(name)
}

/// Is `name` a prototype object a provider declares?
pub(crate) fn is_host_proto(name: &str) -> bool {
    current().is_some_and(|h| {
        h.table.object_kinds.get(name) == Some(&hs::HostObjectKind::Prototype)
            && h.entry(name).is_some_and(|e| e.kind == RealmKind::Prototype)
    })
}

/// Is `name` a global a provider adds to `%Global%`?
pub(crate) fn is_host_global(key: &str) -> bool {
    current().is_some_and(|h| h.table.global_own.iter().any(|o| !o.symbol && o.key == key))
}

/// The host realm installed for one solve; dropping it restores the
/// previous one.
pub(crate) struct Installed(Option<&'static HostRealm>);

pub(crate) fn install(h: Option<&'static HostRealm>) -> Installed {
    Installed(HOST.with(|c| c.replace(h)))
}

impl Drop for Installed {
    fn drop(&mut self) {
        HOST.with(|c| c.set(self.0));
    }
}
