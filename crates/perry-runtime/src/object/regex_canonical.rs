//! Non-observable admission for builtin RegExp operations. ShapeId guards the
//! own key/descriptor layout, indexed loads guard the current values of `exec`
//! and `constructor`, and the existing symbol epoch guards @@replace, @@match,
//! @@split and `RegExp[@@species]`. No getter is invoked here.
use super::{regex_proto_thunks as thunks, ObjectHeader};
use crate::regex::RegExpHeader;
use crate::value::{js_nanbox_pointer, JSValue};
use std::cell::Cell;
use std::sync::atomic::Ordering;

/// A symbol-keyed method on `RegExp.prototype` that a String method looks up
/// before doing anything else, and the builtin that must still be installed
/// there for the lookup to be skipped.
#[derive(Clone, Copy)]
pub(crate) enum Method {
    Replace,
    Match,
    Split,
}
impl Method {
    fn symbol(self) -> &'static str {
        match self {
            Self::Replace => "replace",
            Self::Match => "match",
            Self::Split => "split",
        }
    }
    fn builtin(self) -> *const u8 {
        match self {
            Self::Replace => crate::regex::perex_replace::regexp_thunk as *const u8,
            Self::Match => crate::regex::perex_match_search::match_thunk as *const u8,
            Self::Split => crate::regex::perex_split::regexp_thunk as *const u8,
        }
    }
    fn bit(self) -> u8 {
        1 << self as u8
    }
}

#[derive(Clone, Copy, Default, PartialEq)]
struct Proof {
    shape: u32,
    exec_index: Option<u32>,
    constructor_index: Option<u32>,
    flags: bool,
    symbol_epoch: u64,
    /// `Method::bit`s whose prototype property is the builtin data property.
    methods: u8,
    /// The constructor `species_builtin` was decided for, by address. Only a
    /// scalar identity compared against a live read, never dereferenced.
    species_owner: usize,
    species_builtin: bool,
}
crate::perry_thread_local! {
    // ShapeIds are immutable scalar identities. This record holds no GC edge.
    static PROOF: Cell<Proof> = Cell::new(Proof::default());
}

fn native(value: f64, function: *const u8) -> bool {
    if !super::is_callable_function_value(value) {
        return false;
    }
    let closure =
        crate::value::js_nanbox_get_pointer(value) as *const crate::closure::ClosureHeader;
    crate::closure::js_closure_get_func(closure) == function
}

fn field_index(proto: *mut ObjectHeader, name: &[u8]) -> Option<u32> {
    let keys_view = unsafe { super::object_keys(proto) };
    let keys = keys_view.arr();
    if keys.is_null() {
        return None;
    }
    (0..keys_view.count()).find(|&i| unsafe {
        crate::string::js_string_key_matches_bytes(
            JSValue::from_bits(crate::array::js_array_get_f64(keys, i).to_bits()),
            name,
        )
    })
}

fn refresh(proto: *mut ObjectHeader, shape: u32) -> Proof {
    let data_index = |name: &str| {
        if super::get_accessor_descriptor(proto as usize, name).is_none() {
            field_index(proto, name.as_bytes())
        } else {
            None
        }
    };
    let exec_index = data_index("exec");
    let constructor_index = data_index("constructor");
    let flags = [
        ("flags", thunks::regex_proto_flags_getter as *const u8),
        ("global", thunks::regex_proto_global_getter as *const u8),
        (
            "ignoreCase",
            thunks::regex_proto_ignore_case_getter as *const u8,
        ),
        (
            "multiline",
            thunks::regex_proto_multiline_getter as *const u8,
        ),
        ("dotAll", thunks::regex_proto_dot_all_getter as *const u8),
        ("sticky", thunks::regex_proto_sticky_getter as *const u8),
        ("unicode", thunks::regex_proto_unicode_getter as *const u8),
        (
            "unicodeSets",
            thunks::regex_proto_unicode_sets_getter as *const u8,
        ),
        (
            "hasIndices",
            thunks::regex_proto_has_indices_getter as *const u8,
        ),
    ]
    .iter()
    .all(|&(name, fp)| {
        super::get_accessor_descriptor(proto as usize, name)
            .is_some_and(|accessor| native(f64::from_bits(accessor.get), fp))
    });
    Proof {
        shape,
        exec_index,
        constructor_index,
        flags,
        ..Proof::default()
    }
}

/// Only an untouched RegExp receiver is admitted. Metadata would permit own
/// overrides, descriptors or a custom prototype and takes the generic path.
pub(crate) fn exec(value: f64) -> bool {
    let receiver = JSValue::from_bits(value.to_bits());
    if !receiver.is_pointer() {
        return false;
    }
    let re = receiver.as_pointer::<RegExpHeader>();
    if !crate::regex::is_valid_regex_ptr(re)
        || unsafe { !(*re).meta.is_null() }
        || super::exotic_expando::has_expando_values(re as usize)
    {
        return false;
    }
    if super::prototype_chain::object_static_prototype_known_non_meta(re as usize).is_some() {
        return false;
    }
    let proto = thunks::recorded_regexp_prototype();
    if proto.is_null() {
        return false;
    }
    let shape = unsafe { super::shapes::object_shape_stamp(proto) };
    if shape == 0 {
        return false;
    }
    PROOF.with(|cell| {
        let mut proof = cell.get();
        if proof.shape != shape {
            proof = refresh(proto, shape);
            cell.set(proof);
        }
        proof.exec_index.is_some_and(|index| {
            native(
                f64::from_bits(super::js_object_get_field(proto, index).bits()),
                thunks::regex_proto_exec_thunk as *const u8,
            )
        })
    })
}

/// The symbol-keyed facts, recomputed when the symbol epoch moves. Every
/// symbol-property mutation and every completed collection advances it, so an
/// address recorded here is compared only while the heap is unchanged.
fn with_symbol_facts<R>(f: impl FnOnce(&mut Proof) -> R) -> R {
    PROOF.with(|cell| {
        let before = cell.get();
        let mut proof = before;
        let epoch = crate::symbol::PERRY_SYMBOL_PROPERTY_IC_EPOCH.load(Ordering::Acquire);
        if proof.symbol_epoch != epoch {
            let proto = thunks::recorded_regexp_prototype() as usize;
            proof.methods = 0;
            for method in [Method::Replace, Method::Match, Method::Split] {
                let symbol = crate::symbol::well_known_symbol_if_cached(method.symbol());
                if !symbol.is_null()
                    && crate::symbol::symbol_accessor_descriptor_bits(proto, symbol as usize)
                        .is_none()
                    && crate::symbol::symbol_property_root_bits(proto, symbol as usize)
                        .is_some_and(|v| native(f64::from_bits(v), method.builtin()))
                {
                    proof.methods |= method.bit();
                }
            }
            proof.species_owner = 0;
            proof.species_builtin = false;
            proof.symbol_epoch = epoch;
        }
        let result = f(&mut proof);
        if proof != before {
            cell.set(proof);
        }
        result
    })
}

/// Would `Get(value, @@<method>)` answer the builtin without running code?
/// Requires the whole `exec` proof and the flag accessors too: every caller
/// goes on to consult `flags` and `exec`, which it may then skip as well.
pub(crate) fn method(value: f64, method: Method) -> bool {
    if !exec(value) {
        return false;
    }
    let symbol = crate::symbol::well_known_symbol_if_cached(method.symbol());
    if symbol.is_null() {
        return false;
    }
    let key = js_nanbox_pointer(symbol as i64);
    if unsafe { crate::symbol::js_object_has_own_symbol_property(value, key) } {
        return false;
    }
    with_symbol_facts(|proof| proof.flags && proof.methods & method.bit() != 0)
}

pub(crate) fn replace(value: f64) -> bool {
    method(value, Method::Replace)
}

/// Would String.prototype.split reach the builtin @@split, and would that
/// method's SpeciesConstructor(value, %RegExp%) select the intrinsic, without
/// running code? The second half is `Get(value, "constructor")` reaching the
/// prototype's data property holding the intrinsic `RegExp`, whose own
/// `@@species` is still the builtin accessor returning `this`.
pub(crate) fn split(value: f64) -> bool {
    if !method(value, Method::Split) {
        return false;
    }
    // `method` just refreshed the proof for the current prototype shape.
    let Some(index) = PROOF.with(|cell| cell.get().constructor_index) else {
        return false;
    };
    let proto = thunks::recorded_regexp_prototype();
    let constructor = f64::from_bits(super::js_object_get_field(proto, index).bits());
    if !super::is_callable_function_value(constructor)
        || !thunks::is_intrinsic_regexp_constructor(constructor)
    {
        return false;
    }
    let owner = crate::value::js_nanbox_get_pointer(constructor) as usize;
    with_symbol_facts(|proof| {
        if proof.species_owner != owner {
            let symbol = crate::symbol::well_known_symbol_if_cached("species");
            proof.species_builtin = !symbol.is_null()
                && crate::symbol::symbol_accessor_descriptor_bits(owner, symbol as usize)
                    .is_some_and(|(get, _)| {
                        native(
                            f64::from_bits(get),
                            super::global_this::builtin_species_getter_thunk as *const u8,
                        )
                    });
            proof.species_owner = owner;
        }
        proof.species_builtin
    })
}
