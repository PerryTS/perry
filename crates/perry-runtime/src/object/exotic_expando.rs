//! Exotic own properties use ordinary holder shapes and value slots.
//! All exotic cells reach their bag through ObjectMeta.expando. Neither path stores descriptors separately from the holder keys.

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExoticKind {
    Date,
    Error,
    /// Temporal's native payload and metadata have independent ownership.
    Temporal,
    /// Promise properties live on its traced metadata bag.
    Promise,
    /// Collection properties live on their traced metadata bags.
    Map,
    /// `Set` analogue of `Map` (a `SetHeader` cell).
    Set,
}

/// Classify `addr` as a Date cell, RegExp header, or Error header. Returns
/// `None` for everything else (including the small-handle band). Every arm is
/// selected directly by its authoritative `GcHeader` kind.
///
/// #11364: the header read alone is not proof that `addr` is a Perry cell.
/// `try_read_gc_header` checks only the address's magnitude and alignment, so
/// a native `Box` under `POINTER_TAG` (an `AsyncResource` backing, #10926)
/// passes it, and its `addr - 8` word is whatever the allocator placed in
/// front of the `Box`. When that word looked like a `GC_TYPE_MAP` header, the
/// Box was classified as a Map and `exotic_set_property`'s descriptor probe
/// read the Map `meta` field at `addr + 0x18` -- past the Box's 24 bytes, the
/// next allocation's first word, a `4.0` -- and dereferenced it: a 1-in-210
/// SIGSEGV at `0x4010000000000010`. An exotic verdict therefore also requires
/// allocator ownership ([`try_read_tracked_gc_header`]), and every consumer
/// that then reads owner memory on its strength (the descriptor summary via
/// `cell_meta_slot`, the `OBJ_FLAG_NO_EXTEND` bit, the RegExp `lastIndex`
/// slot) inherits that proof. Ordinary objects and every other non-exotic
/// type still answer `None` from the cheap read, so only a receiver that
/// already claims an exotic type pays for the ownership lookup.
///
/// [`try_read_tracked_gc_header`]: crate::value::addr_class::try_read_tracked_gc_header
pub(crate) fn exotic_expando_kind(addr: usize) -> Option<ExoticKind> {
    let gc = unsafe { crate::value::addr_class::try_read_gc_header(addr) }?;
    let claimed = exotic_kind_of_gc_type(gc.obj_type)?;
    // The claim came from an unproven read; answer it only for an address the
    // allocator owns (arena range or exact malloc-registry hit).
    unsafe { crate::value::addr_class::try_read_tracked_gc_header(addr) }?;
    Some(claimed)
}

/// The exotic kind a managed header type denotes. Only a type read from an
/// allocator-owned header (`try_read_tracked_gc_header`) proves the cell's
/// kind; a caller that already holds one decides from it without a second
/// read.
pub(crate) fn exotic_kind_of_gc_type(obj_type: u8) -> Option<ExoticKind> {
    match obj_type {
        crate::gc::GC_TYPE_DATE_CELL => Some(ExoticKind::Date),
        crate::gc::GC_TYPE_ERROR => Some(ExoticKind::Error),
        crate::gc::GC_TYPE_TEMPORAL => Some(ExoticKind::Temporal),
        crate::gc::GC_TYPE_PROMISE => Some(ExoticKind::Promise),
        crate::gc::GC_TYPE_MAP => Some(ExoticKind::Map),
        crate::gc::GC_TYPE_SET => Some(ExoticKind::Set),
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

fn expando_store(addr: usize, key: &str, bits: u64) {
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let bag = property_bag_ensure(addr);
        let key = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
        super::object_ops::define_property_force_store_value(bag, key, f64::from_bits(bits));
    }
}

fn expando_lookup(addr: usize, key: &str) -> Option<u64> {
    unsafe {
        let bag = property_bag(addr);
        if bag.is_null() {
            return None;
        }
        let keys = super::object_keys(bag);
        let slot =
            super::keys_find_property_slot_by_bytes(keys, keys.count(), key.as_bytes())?;
        Some(super::key_attrs::object_slot_data(bag, slot).bits())
    }
}

fn expando_remove(addr: usize, key: &str) -> bool {
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let bag = property_bag(addr);
        if bag.is_null() {
            return false;
        }
        let keys = super::object_keys(bag);
        if super::keys_find_property_slot_by_bytes(keys, keys.count(), key.as_bytes())
            .is_none()
        {
            return false;
        }
        let key = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
        super::js_object_delete_field(bag, key) != 0
    }
}

/// Kind-dispatched own data-property store: Error delegates to the
/// traced property bag, just like the other exotic cells.
pub(crate) fn value_store(_kind: ExoticKind, addr: usize, key: &str, bits: u64) {
    // #10943: this is where an exotic cell ACTUALLY takes a named property,
    // whatever lowering asked for it. Arming at `field_set_by_name`'s exotic
    // gauntlet missed the common spelling outright: `m.get = () => 1` on a
    // proven Map local lowers to `js_put_value_set_dyn_ic`, which never enters
    // that gauntlet, so the guard's predicate answered "no own override" and
    // the builtin still won. Arming at the store itself is the funnel the
    // gauntlet was chosen to approximate.
    crate::object::own_override::note_exotic_named_prop_install(addr);
    expando_store(addr, key, bits);
}

pub(crate) fn value_lookup(_kind: ExoticKind, addr: usize, key: &str) -> Option<u64> {
    expando_lookup(addr, key)
}

pub(crate) fn value_remove(_kind: ExoticKind, addr: usize, key: &str) -> bool {
    expando_remove(addr, key)
}

fn value_keys(_kind: ExoticKind, addr: usize) -> Vec<String> {
    unsafe { super::descriptor_state::holder_key_names(property_bag(addr), false) }
}

#[cfg(test)]
pub(crate) fn test_define_exotic_property(addr: usize, key: &str, value_bits: u64) {
    expando_store(addr, key, value_bits);
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

    {
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
    {
        // Temporal values have 8 distinct prototypes (resolved via brand
        // dispatch, not a single named builtin prototype), and their built-in
        // accessors are served by the Temporal getter path, not the generic
        // accessor side table — so there is no prototype-accessor setter to
        // consult here. Skip straight to the own-property store.
        let proto_name = match kind {
            ExoticKind::Date => "Date",
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
    {
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
        || (super::get_accessor_descriptor(addr, name).is_some())
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
    // Compiler-owned storage (a class's captured locals on an Error subclass
    // instance) is not an own property, as on ordinary objects.
    keys.retain(|k| !super::is_internal_runtime_key(k));
    {
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
/// retained from the current state when REDEFINING. Returns `false` (the
/// definition's verdict) on forbidden non-configurable redefines and
/// non-extensible additions, before any mutation.
pub(crate) unsafe fn exotic_define_own_property(
    addr: usize,
    kind: ExoticKind,
    name: &str,
    descriptor: &crate::object::object_ops::DescView<'_>,
) -> bool {
    // Error instances expose `message`/`stack` as builtin own properties
    // (writable, non-enumerable, configurable) even before any user write.
    let is_error_builtin = kind == ExoticKind::Error && matches!(name, "message" | "stack");
    let existing_accessor = { super::get_accessor_descriptor(addr, name) };
    let existing_value = value_lookup(kind, addr, name);
    let exists = is_error_builtin || existing_accessor.is_some() || existing_value.is_some();

    let cur_attrs = if exists {
        Some(super::get_property_attrs(addr, name).unwrap_or({
            if is_error_builtin {
                super::PropertyAttrs::new(true, false, true)
            } else {
                super::PropertyAttrs::new(true, true, true)
            }
        }))
    } else {
        None
    };

    let has_get = descriptor.has_named(b"get");
    let has_set = descriptor.has_named(b"set");
    let has_value = descriptor.has_named(b"value");
    let has_writable = descriptor.has_named(b"writable");
    let has_enumerable = descriptor.has_named(b"enumerable");
    let has_configurable = descriptor.has_named(b"configurable");
    let read_bool = |field: &[u8]| -> bool {
        let v = descriptor.read_named(field);
        crate::value::js_is_truthy(f64::from_bits(v.bits())) != 0
    };

    if let Some(cur) = cur_attrs {
        if !cur.configurable() {
            let cur_value = existing_value
                .map(f64::from_bits)
                .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
            if !super::nonconfigurable_redefine_allowed(
                cur,
                existing_accessor,
                cur_value,
                descriptor,
            ) {
                return false;
            }
        }
    } else {
        let gc = (addr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc)._reserved & crate::gc::OBJ_FLAG_NO_EXTEND != 0 {
            return false;
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
        let get_field = descriptor.read_named(b"get");
        let set_field = descriptor.read_named(b"set");
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
        // The accessor pair has replaced the data in the same holder slot.
        super::set_property_attrs(
            addr,
            name.to_string(),
            super::PropertyAttrs::new(false, enumerable, configurable),
        );
        return true;
    }

    if existing_accessor.is_some() && (has_value || has_writable) {
        // Accessor → data conversion.
        super::clear_accessor_descriptor(addr, name);
    }
    if has_value {
        let v = descriptor.read_named(b"value");
        value_store(kind, addr, name, v.bits());
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
    true
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

#[cfg(test)]
mod tests {
    use super::*;

    /// #11364: a native `Box` under `POINTER_TAG` whose preceding allocator
    /// word looks like a `GC_TYPE_MAP` header must not be classified as a Map.
    /// The crash shape: an `AsyncResource` backing (24 bytes) was taken for a
    /// Map, and the `[[Set]]` descriptor probe read the Map `meta` field at
    /// `+0x18` -- the NEXT allocation's `4.0` -- and dereferenced
    /// `0x4010000000000010`. The fixture reproduces exactly that: a spoofed
    /// Map header in front of an untracked owner whose `meta` word is `4.0`.
    /// Without the ownership gate the first assertion fails (and the set path
    /// below would SIGSEGV); with it the receiver is never exotic.
    #[test]
    fn untracked_owner_with_spoofed_map_header_is_not_exotic() {
        let four = 4.0f64.to_bits();
        let mut words = Box::new([four; 16]);
        words[0] = crate::gc::GC_TYPE_MAP as u64 | ((16 * 8) << 32);
        let owner = &words[1] as *const u64 as usize;
        let meta_offset = std::mem::offset_of!(crate::map::MapHeader, meta);
        assert_eq!(
            words[1 + meta_offset / 8],
            four,
            "the spoofed Map's meta word must be the crash's 4.0"
        );
        // Precondition: the magnitude-checked reader alone WOULD call this a
        // Map. Without it the test proves nothing.
        let hdr = unsafe { crate::value::addr_class::try_read_gc_header(owner) };
        assert_eq!(
            hdr.map(|h| h.obj_type),
            Some(crate::gc::GC_TYPE_MAP),
            "fixture must look like a Map cell to the magnitude-checked reader"
        );

        assert!(
            exotic_expando_kind(owner).is_none(),
            "an allocator-untracked owner was classified as an exotic cell"
        );
        let target = crate::value::js_nanbox_pointer(owner as i64);
        assert!(exotic_expando_kind_of_value(target).is_none());

        // The crash path itself: with descriptors in use, the `[[Set]]` arm
        // used to probe the descriptor summary through the spoofed `meta`.
        let name_ptr = crate::string::js_string_from_bytes(b"label".as_ptr(), 5);
        let key = crate::value::js_nanbox_string(name_ptr as i64);
        assert!(exotic_put_value_set(target, key, 42.0, target, 1).is_none());
        assert!(
            words[1..].iter().all(|w| *w == four),
            "owner memory was written"
        );
        drop(words);
    }

    /// Positive control for the ownership gate: genuine exotic cells, arena
    /// (Map/Set/Date/Error) and malloc-tracked (Promise), still classify.
    #[test]
    fn genuine_exotic_cells_still_classify() {
        let map = crate::map::js_map_alloc(4) as usize;
        let set = crate::set::js_set_alloc(4) as usize;
        let error = crate::error::js_error_new() as usize;
        let promise = crate::promise::js_promise_new() as usize;
        let date = crate::date_hook_slots::js_date_new();
        assert!(exotic_expando_kind(map) == Some(ExoticKind::Map));
        assert!(exotic_expando_kind(set) == Some(ExoticKind::Set));
        assert!(exotic_expando_kind(error) == Some(ExoticKind::Error));
        assert!(exotic_expando_kind(promise) == Some(ExoticKind::Promise));
        assert!(matches!(
            exotic_expando_kind_of_value(date),
            Some((_, ExoticKind::Date))
        ));
    }
}

/// Every exotic owner carries its property bag through its own metadata.
pub(crate) unsafe fn property_bag(addr: usize) -> *mut super::ObjectHeader {
    if crate::value::addr_class::try_read_tracked_gc_header(addr).is_none() {
        return std::ptr::null_mut();
    }
    super::cell_expando_get(addr).unwrap_or(std::ptr::null_mut())
}

pub(crate) unsafe fn property_bag_ensure(addr: usize) -> *mut super::ObjectHeader {
    assert!(crate::value::addr_class::try_read_tracked_gc_header(addr).is_some());
    super::cell_expando_ensure(addr).expect("exotic cell must own a metadata edge")
}
