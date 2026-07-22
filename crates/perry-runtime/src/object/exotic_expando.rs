//! Expando (user-defined own) properties for exotic instances whose heap
//! representation is NOT an `ObjectHeader`: `Date` (an 8-byte `DateCell`),
//! `RegExp` (a `RegExpHeader`), and `Error` (an `ErrorHeader`). Plain
//! property writes on these previously either no-op'd (Date guard in
//! `js_object_set_field_by_name`) or wrote through garbage field offsets
//! (RegExp), and reads fell through to cell-as-`ObjectHeader` derefs that
//! could segfault. test262 exercises both directions heavily: exotic
//! instances as `Object.defineProperty` targets AND as the *attributes*
//! object (`dateObj.value = "x"; defineProperty(o, k, dateObj)`).
//!
//! Date/RegExp values are stored as NaN-boxed bits keyed by the cell
//! address, in insertion order (spec OrdinaryOwnPropertyKeys for string
//! keys). Error values delegate to the pre-existing `ERROR_USER_PROPS`
//! side table so the dedicated error get/set arms and `assert.throws`
//! consumers keep seeing one store. Attribute flags and accessor get/set
//! closures piggyback on the generic side tables (`PROPERTY_DESCRIPTORS` /
//! `ACCESSOR_DESCRIPTORS`), which are already keyed by raw address.
//!
//! GC: all three cell kinds are non-movable, so the address key is stable
//! for the cell's lifetime. Stored values are kept alive via a registered
//! mutable root scanner. Address reuse after a sweep is handled by clearing
//! the table slot at allocation time (`expando_clear_on_alloc`).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExoticKind {
    Date,
    RegExp,
    Error,
    /// A `Temporal.*` reference value (any of the 8 brand sub-kinds). Like
    /// `Date`, it is a non-movable, pointer-free cell that is NOT an
    /// `ObjectHeader`, so user-defined own properties (`Object.defineProperty`,
    /// plain assignment — e.g. test262's `instance.constructor = …` and the
    /// `checkToTemporalCalendarFastPath` getter probes) must live in the
    /// side table rather than being written through a bit-cast that corrupts
    /// the boxed payload. Built-in getters (`duration.years`, `plainDate.month`)
    /// stay in the Temporal dispatch path; only true expando keys land here.
    Temporal,
    /// A `Promise` is a `GC_TYPE_PROMISE` cell, NOT an `ObjectHeader`, so a
    /// plain property write (`p.status = "pending"`, `Object.assign(p, …)`)
    /// must land in the side table rather than being bit-cast through the
    /// `ObjectHeader` field path (which silently dropped the write — reads came
    /// back `undefined`). Libraries attach bookkeeping to the promise object
    /// itself: @tanstack/query-core's `pendingThenable()` stores `status` /
    /// `value` on the promise and gates its retryer on `thenable.status`, so a
    /// dropped expando left `isResolved()` permanently true and the fetch never
    /// resolved (#5142). Built-in `then`/`catch`/`finally` reads stay on the
    /// promise dispatch path; only true expando keys land here. Unlike Date /
    /// Temporal, a promise is movable, so its expando entry is rekeyed on GC
    /// move via `exotic_expando_owner_moved`.
    Promise,
    /// A `Map` is a `MapHeader` cell, NOT an `ObjectHeader` — a plain expando
    /// write (`memoized.cache.custom = x` on a lodash-memoize Map cache) must
    /// land in the side table rather than being bit-cast through the
    /// `ObjectHeader` field path, which overwrote collection-internal fields
    /// (and enumeration then read those bytes back as a garbage `keys_array`
    /// pointer → SIGSEGV). Collection DATA stays in the map natives; only
    /// true expando keys land here. Movable — rekeyed on GC move like
    /// Promise.
    Map,
    /// `Set` analogue of `Map` (a `SetHeader` cell).
    Set,
}

/// Classify `addr` as a Date cell, RegExp header, or Error header. Returns
/// `None` for everything else (including the small-handle band). One
/// `GcHeader` read; the RegExp set probe only runs for `GC_TYPE_OBJECT`.
pub(crate) fn exotic_expando_kind(addr: usize) -> Option<ExoticKind> {
    let gc = unsafe { crate::value::addr_class::try_read_gc_header(addr) }?;
    match gc.obj_type {
        crate::gc::GC_TYPE_DATE_CELL => Some(ExoticKind::Date),
        crate::gc::GC_TYPE_ERROR => Some(ExoticKind::Error),
        crate::gc::GC_TYPE_TEMPORAL => Some(ExoticKind::Temporal),
        crate::gc::GC_TYPE_PROMISE => Some(ExoticKind::Promise),
        crate::gc::GC_TYPE_MAP => Some(ExoticKind::Map),
        crate::gc::GC_TYPE_SET => Some(ExoticKind::Set),
        crate::gc::GC_TYPE_OBJECT if crate::regex::is_regex_pointer(addr as *const u8) => {
            Some(ExoticKind::RegExp)
        }
        _ => None,
    }
}

/// Classify a NaN-boxed (or raw-I64) value as an exotic-expando receiver.
/// Returns the cleaned address alongside the kind.
pub(crate) fn exotic_expando_kind_of_value(value: f64) -> Option<(usize, ExoticKind)> {
    let bits = value.to_bits();
    let top16 = bits >> 48;
    let addr = if top16 == 0x7FFD {
        (bits & crate::value::POINTER_MASK) as usize
    } else if top16 == 0 {
        bits as usize
    } else {
        return None;
    };
    if addr == 0 {
        return None;
    }
    exotic_expando_kind(addr).map(|kind| (addr, kind))
}

thread_local! {
    /// addr -> insertion-ordered (key, nanboxed value bits) pairs (Date/RegExp).
    static EXOTIC_EXPANDO: RefCell<HashMap<usize, Vec<(String, u64)>>> =
        RefCell::new(HashMap::new());
    /// Fast-path gate so hot get/set paths skip the map lookup until the
    /// first expando is installed on this thread.
    static EXPANDO_IN_USE: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn expando_in_use() -> bool {
    EXPANDO_IN_USE.with(|c| c.get())
}

fn expando_store(addr: usize, key: &str, bits: u64) {
    EXPANDO_IN_USE.with(|c| c.set(true));
    EXOTIC_EXPANDO.with(|m| {
        let mut map = m.borrow_mut();
        let entries = map.entry(addr).or_default();
        if let Some(slot) = entries.iter_mut().find(|(k, _)| k == key) {
            slot.1 = bits;
        } else {
            entries.push((key.to_string(), bits));
        }
    });
}

fn expando_lookup(addr: usize, key: &str) -> Option<u64> {
    if !expando_in_use() {
        return None;
    }
    EXOTIC_EXPANDO.with(|m| {
        m.borrow()
            .get(&addr)
            .and_then(|entries| entries.iter().find(|(k, _)| k == key).map(|(_, v)| *v))
    })
}

fn expando_remove(addr: usize, key: &str) -> bool {
    if !expando_in_use() {
        return false;
    }
    EXOTIC_EXPANDO.with(|m| {
        let mut map = m.borrow_mut();
        if let Some(entries) = map.get_mut(&addr) {
            let before = entries.len();
            entries.retain(|(k, _)| k != key);
            return entries.len() != before;
        }
        false
    })
}

/// Kind-dispatched own data-property store: Error delegates to the
/// pre-existing `ERROR_USER_PROPS` table, Date/RegExp use `EXOTIC_EXPANDO`.
pub(crate) fn value_store(kind: ExoticKind, addr: usize, key: &str, bits: u64) {
    match kind {
        ExoticKind::Error => {
            crate::node_submodules::set_error_user_prop(addr, key, f64::from_bits(bits))
        }
        _ => expando_store(addr, key, bits),
    }
}

pub(crate) fn value_lookup(kind: ExoticKind, addr: usize, key: &str) -> Option<u64> {
    match kind {
        ExoticKind::Error => {
            crate::node_submodules::error_user_prop(addr, key).map(|v| v.to_bits())
        }
        _ => expando_lookup(addr, key),
    }
}

pub(crate) fn value_remove(kind: ExoticKind, addr: usize, key: &str) -> bool {
    match kind {
        ExoticKind::Error => crate::node_submodules::remove_error_user_prop(addr, key),
        _ => expando_remove(addr, key),
    }
}

fn value_keys(kind: ExoticKind, addr: usize) -> Vec<String> {
    match kind {
        ExoticKind::Error => crate::node_submodules::error_user_props(addr)
            .into_iter()
            .map(|(k, _)| k)
            .collect(),
        _ => {
            if !expando_in_use() {
                return Vec::new();
            }
            EXOTIC_EXPANDO.with(|m| {
                m.borrow()
                    .get(&addr)
                    .map(|entries| entries.iter().map(|(k, _)| k.clone()).collect())
                    .unwrap_or_default()
            })
        }
    }
}

/// Drop any stale expando entries left at `addr` by a previous (collected)
/// cell. Called from Date / RegExp allocation so address reuse can't leak
/// the old instance's properties onto the new one.
pub(crate) fn expando_clear_on_alloc(addr: usize) {
    if !expando_in_use() {
        return;
    }
    EXOTIC_EXPANDO.with(|m| {
        m.borrow_mut().remove(&addr);
    });
}

/// Death pruning (2026-07-09 GC audit wave 2): the root scanner
/// (`scan_exotic_expando_roots_mut`) strongly roots EVERY owner's values,
/// dead owners included, so a dead Date/RegExp/Promise/Map/Set's expando
/// value graph was immortal until the exact address happened to be handed
/// to a new cell of the same kind (`expando_clear_on_alloc`). Prune entries
/// whose owner cell is provably dead instead. `is_dead_owner` is one of the
/// GC's deadness predicates (`gc::dead_owner`). Note: non-movable Date /
/// Temporal cells that die PINNED are skipped by the predicate's pinned
/// check and remain covered by the clear-on-alloc path.
pub(crate) fn prune_dead_exotic_expando_owners(is_dead_owner: &dyn Fn(usize) -> bool) {
    if !expando_in_use() {
        return;
    }
    EXOTIC_EXPANDO.with(|m| {
        let mut map = m.borrow_mut();
        if !map.is_empty() {
            map.retain(|owner, _| !is_dead_owner(*owner));
        }
    });
}

#[cfg(test)]
pub(crate) fn test_seed_exotic_expando_entry(addr: usize, key: &str, value_bits: u64) {
    EXPANDO_IN_USE.with(|c| c.set(true));
    EXOTIC_EXPANDO.with(|m| {
        m.borrow_mut()
            .entry(addr)
            .or_default()
            .push((key.to_string(), value_bits));
    });
}

#[cfg(test)]
pub(crate) fn test_exotic_expando_entry_exists(addr: usize) -> bool {
    EXOTIC_EXPANDO.with(|m| m.borrow().contains_key(&addr))
}

/// Rekey a movable exotic cell's expando entry after the GC relocates it from
/// `old_addr` to `new_addr`. Date / RegExp / Temporal cells are non-movable so
/// this never fires for them, but a `Promise` (`GC_TYPE_PROMISE`) is movable —
/// without this, a `.then()`-chained thenable that survives a GC move would
/// lose the `status`/`value` expandos it was gated on. Stored expando *values*
/// are already rewritten by `scan_exotic_expando_roots_mut`; this migrates the
/// owner *key*. Wired via `GcMoveHookKind::ExoticExpandoOwner`.
pub(crate) fn exotic_expando_owner_moved(old_addr: usize, new_addr: usize) {
    if !expando_in_use() || old_addr == new_addr {
        return;
    }
    EXOTIC_EXPANDO.with(|m| {
        let mut map = m.borrow_mut();
        if let Some(entries) = map.remove(&old_addr) {
            map.insert(new_addr, entries);
        }
    });
}

/// `[[Set]]` on a Date/RegExp/Error instance. Honors accessor descriptors
/// and attribute writability from the generic side tables, plus the RegExp
/// `lastIndex` header slot and non-extensibility for new keys. Returns
/// `false` when the write is rejected (caller decides strict-mode throw).
pub(crate) unsafe fn exotic_set_property(
    addr: usize,
    kind: ExoticKind,
    name: &str,
    value: f64,
    receiver: f64,
) -> bool {
    // RegExp `lastIndex` is a writable data property living in the header.
    if kind == ExoticKind::RegExp && name == "lastIndex" {
        if let Some(attrs) = super::get_property_attrs(addr, name) {
            if !attrs.writable() {
                return false;
            }
        }
        crate::regex::js_regexp_set_last_index(addr as *mut crate::regex::RegExpHeader, value);
        return true;
    }
    if super::descriptors_in_use() {
        if let Some(acc) = super::get_accessor_descriptor(addr, name) {
            if acc.set == 0 {
                return false;
            }
            super::invoke_accessor_setter(acc.set, receiver, value);
            return true;
        }
        if let Some(attrs) = super::get_property_attrs(addr, name) {
            if !attrs.writable() {
                return false;
            }
            value_store(kind, addr, name, value.to_bits());
            return true;
        }
    }
    if value_lookup(kind, addr, name).is_some() {
        value_store(kind, addr, name, value.to_bits());
        return true;
    }
    // No own property: an accessor inherited from the builtin prototype
    // (`Object.defineProperty(Date.prototype, "prop", {set})`) consumes the
    // write — the setter runs with the instance receiver and NO own expando
    // is created (spec OrdinarySetWithOwnDescriptor walking the chain).
    if super::descriptors_in_use() {
        // Temporal values have 8 distinct prototypes (resolved via brand
        // dispatch, not a single named builtin prototype), and their built-in
        // accessors are served by the Temporal getter path, not the generic
        // accessor side table — so there is no prototype-accessor setter to
        // consult here. Skip straight to the own-property store.
        let proto_name = match kind {
            ExoticKind::Date => "Date",
            ExoticKind::RegExp => "RegExp",
            ExoticKind::Error => "Error",
            ExoticKind::Temporal => "",
            // Promise.prototype's `then`/`catch`/`finally` are methods, not
            // generic accessors in the side table, so there is no
            // prototype-accessor setter to consult; store the own expando.
            ExoticKind::Promise => "",
            // Map/Set prototype members are methods + the `size` accessor,
            // all served by their native dispatch paths — store the own
            // expando directly.
            ExoticKind::Map | ExoticKind::Set => "",
        };
        let proto = if proto_name.is_empty() {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        } else {
            super::builtin_prototype_value(proto_name)
        };
        let proto_bits = proto.to_bits();
        if (proto_bits >> 48) == 0x7FFD {
            let proto_addr = (proto_bits & crate::value::POINTER_MASK) as usize;
            if proto_addr != 0 {
                if let Some(acc) = super::get_accessor_descriptor(proto_addr, name) {
                    if acc.set == 0 {
                        return false;
                    }
                    super::invoke_accessor_setter(acc.set, receiver, value);
                    return true;
                }
            }
        }
    }
    // New property: reject when the instance was made non-extensible.
    let gc = (addr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*gc)._reserved & crate::gc::OBJ_FLAG_NO_EXTEND != 0 {
        return false;
    }
    value_store(kind, addr, name, value.to_bits());
    true
}

/// `[[Get]]` of an own expando / accessor property on an exotic instance.
/// Returns `None` when no own property exists (caller continues with
/// builtin header props / prototype methods).
pub(crate) unsafe fn exotic_get_own_property(
    addr: usize,
    kind: ExoticKind,
    name: &str,
    receiver: f64,
) -> Option<f64> {
    if kind != ExoticKind::Error && !expando_in_use() && !super::descriptors_in_use() {
        return None;
    }
    if super::descriptors_in_use() {
        if let Some(acc) = super::get_accessor_descriptor(addr, name) {
            if acc.get == 0 {
                return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
            }
            return Some(f64::from_bits(
                super::invoke_accessor_getter(acc.get, receiver).bits(),
            ));
        }
    }
    value_lookup(kind, addr, name).map(f64::from_bits)
}

/// True when (addr, name) resolves to an own expando data property OR an
/// installed accessor descriptor (HasOwnProperty for exotic instances).
pub(crate) fn exotic_has_own_property(kind: ExoticKind, addr: usize, name: &str) -> bool {
    value_lookup(kind, addr, name).is_some()
        || (super::descriptors_in_use() && super::get_accessor_descriptor(addr, name).is_some())
}

/// Default enumerability for an exotic instance's own key when no explicit
/// attribute entry exists. A built-in slot that the spec defines as
/// non-enumerable (Error's `message`/`stack`) stays non-enumerable even after a
/// plain `err.message = x` write (which is a `[[Set]]` — it never changes
/// attributes); a user-added expando (`err.foo = 1`) defaults to enumerable.
pub(crate) fn exotic_default_enumerable(kind: ExoticKind, name: &str) -> bool {
    !(kind == ExoticKind::Error && matches!(name, "message" | "stack"))
}

/// Own expando string keys: data props in insertion order, then
/// accessor-only keys. Optionally filtered to enumerable ones.
pub(crate) fn exotic_own_keys(kind: ExoticKind, addr: usize, enumerable_only: bool) -> Vec<String> {
    let mut keys = value_keys(kind, addr);
    if super::descriptors_in_use() {
        for key in super::accessor_descriptor_keys_for_obj(addr) {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
    }
    if enumerable_only {
        keys.retain(|k| {
            super::get_property_attrs(addr, k)
                .map(|a| a.enumerable())
                .unwrap_or_else(|| exotic_default_enumerable(kind, k))
        });
    }
    keys
}

/// `[[DefineOwnProperty]]` on a Date/RegExp/Error instance
/// (`Object.defineProperty(dateObj, "prop", {...})`). Mirrors the ordinary
/// ValidateAndApplyPropertyDescriptor flow against the side tables: absent
/// fields default to `false`/`undefined` for NEW properties and are
/// retained from the current state when REDEFINING. Throws TypeError on
/// forbidden non-configurable redefines and non-extensible additions.
pub(crate) unsafe fn exotic_define_own_property(
    addr: usize,
    kind: ExoticKind,
    name: &str,
    descriptor_value: f64,
) {
    let is_last_index = kind == ExoticKind::RegExp && name == "lastIndex";
    // Error instances expose `message`/`stack` as builtin own properties
    // (writable, non-enumerable, configurable) even before any user write.
    let is_error_builtin = kind == ExoticKind::Error && matches!(name, "message" | "stack");
    let existing_accessor = if super::descriptors_in_use() {
        super::get_accessor_descriptor(addr, name)
    } else {
        None
    };
    let existing_value = value_lookup(kind, addr, name);
    let exists = is_last_index
        || is_error_builtin
        || existing_accessor.is_some()
        || existing_value.is_some();

    let cur_attrs = if exists {
        Some(super::get_property_attrs(addr, name).unwrap_or({
            if is_last_index {
                // lastIndex: writable, non-enumerable, non-configurable.
                super::PropertyAttrs::new(true, false, false)
            } else if is_error_builtin {
                super::PropertyAttrs::new(true, false, true)
            } else {
                super::PropertyAttrs::new(true, true, true)
            }
        }))
    } else {
        None
    };

    let has_get = super::desc_has_field(descriptor_value, b"get");
    let has_set = super::desc_has_field(descriptor_value, b"set");
    let has_value = super::desc_has_field(descriptor_value, b"value");
    let has_writable = super::desc_has_field(descriptor_value, b"writable");
    let has_enumerable = super::desc_has_field(descriptor_value, b"enumerable");
    let has_configurable = super::desc_has_field(descriptor_value, b"configurable");
    let read_bool = |field: &[u8]| -> bool {
        let v = super::desc_read_field(descriptor_value, field);
        crate::value::js_is_truthy(f64::from_bits(v.bits())) != 0
    };

    if let Some(cur) = cur_attrs {
        if !cur.configurable() {
            let cur_value = existing_value.map(f64::from_bits).unwrap_or_else(|| {
                if is_last_index {
                    f64::from_bits((*(addr as *const crate::regex::RegExpHeader)).last_index)
                } else {
                    f64::from_bits(crate::value::TAG_UNDEFINED)
                }
            });
            super::validate_nonconfigurable_redefine(
                name,
                cur,
                existing_accessor,
                cur_value,
                descriptor_value,
                None,
            );
        }
    } else {
        let gc = (addr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc)._reserved & crate::gc::OBJ_FLAG_NO_EXTEND != 0 {
            super::throw_object_type_error_with_suffix(
                "Cannot define property ",
                &format!("{name}, object is not extensible"),
            );
        }
    }

    // Defaults: retained from current state on redefine, false for new.
    let defaults = cur_attrs.unwrap_or(super::PropertyAttrs::new(false, false, false));
    let writable = if has_writable {
        read_bool(b"writable")
    } else {
        defaults.writable()
    };
    let enumerable = if has_enumerable {
        read_bool(b"enumerable")
    } else {
        defaults.enumerable()
    };
    let configurable = if has_configurable {
        read_bool(b"configurable")
    } else {
        defaults.configurable()
    };

    if has_get || has_set {
        let get_field = super::desc_read_field(descriptor_value, b"get");
        let set_field = super::desc_read_field(descriptor_value, b"set");
        let merged = super::AccessorDescriptor {
            get: if has_get {
                if get_field.is_undefined() {
                    0
                } else {
                    get_field.bits()
                }
            } else {
                existing_accessor.map(|a| a.get).unwrap_or(0)
            },
            set: if has_set {
                if set_field.is_undefined() {
                    0
                } else {
                    set_field.bits()
                }
            } else {
                existing_accessor.map(|a| a.set).unwrap_or(0)
            },
        };
        super::set_accessor_descriptor(addr, name.to_string(), merged);
        // Data → accessor conversion drops the stored value.
        value_remove(kind, addr, name);
        super::set_property_attrs(
            addr,
            name.to_string(),
            super::PropertyAttrs::new(false, enumerable, configurable),
        );
        return;
    }

    if existing_accessor.is_some() && (has_value || has_writable) {
        // Accessor → data conversion.
        super::clear_accessor_descriptor(addr, name);
    }
    if has_value {
        let v = super::desc_read_field(descriptor_value, b"value");
        if is_last_index {
            crate::regex::js_regexp_set_last_index(
                addr as *mut crate::regex::RegExpHeader,
                f64::from_bits(v.bits()),
            );
        } else {
            value_store(kind, addr, name, v.bits());
        }
    } else if !exists {
        // New property with absent [[Value]] reads as undefined.
        value_store(kind, addr, name, crate::value::TAG_UNDEFINED);
    } else if existing_accessor.is_some() {
        // Accessor → data with no value: slot becomes undefined.
        value_store(kind, addr, name, crate::value::TAG_UNDEFINED);
    }
    super::set_property_attrs(
        addr,
        name.to_string(),
        super::PropertyAttrs::new(writable, enumerable, configurable),
    );
}

/// Assignment `PutValue` arm for exotic receivers, used by
/// `js_put_value_set`: a Date/RegExp/Error target must not flow into the
/// ordinary `ObjectHeader` set path (bit-cast / corruption). Returns
/// `Some(assigned value)` when the receiver was exotic and handled here,
/// `None` to continue with the ordinary path. Throws on a rejected strict
/// write.
pub(crate) fn exotic_put_value_set(
    target: f64,
    property_key: f64,
    value: f64,
    receiver: f64,
    strict: i32,
) -> Option<f64> {
    let (addr, kind) = exotic_expando_kind_of_value(target)?;
    if unsafe { crate::symbol::js_is_symbol(property_key) } != 0 {
        unsafe { crate::symbol::js_object_set_symbol_property(target, property_key, value) };
        return Some(value);
    }
    let name = unsafe { super::metadata_key_to_string(property_key) }?;
    let ok = unsafe { exotic_set_property(addr, kind, &name, value, receiver) };
    if !ok && strict != 0 {
        crate::error::throw_immutable_write(0, &name);
    }
    Some(value)
}

/// GC mutable-root scanner: keeps expando values alive (and rewrites them if
/// the collector relocates the referenced heap objects).
pub fn scan_exotic_expando_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    EXOTIC_EXPANDO.with(|m| {
        let mut map = m.borrow_mut();
        for (_, entries) in map.iter_mut() {
            for (_, bits) in entries.iter_mut() {
                visitor.visit_nanbox_u64_slot(bits);
            }
        }
    });
}
