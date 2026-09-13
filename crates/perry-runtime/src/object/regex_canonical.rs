//! Non-observable admission for builtin RegExp operations. ShapeId guards the
//! own key/descriptor layout, an indexed load guards exec's current value, and
//! the existing symbol epoch guards @@replace. No getter is invoked here.
use super::{regex_proto_thunks as thunks, ObjectHeader};
use crate::regex::RegExpHeader;
use crate::value::{js_nanbox_pointer, JSValue};
use std::cell::Cell;
use std::sync::atomic::Ordering;

#[derive(Clone, Copy, Default)]
struct Proof {
    shape: u32,
    exec_index: Option<u32>,
    flags: bool,
    symbol_epoch: u64,
    replace: bool,
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
    let keys = unsafe { super::object_keys_array(proto) };
    if keys.is_null() {
        return None;
    }
    (0..crate::array::js_array_length(keys)).find(|&i| unsafe {
        crate::string::js_string_key_matches_bytes(
            JSValue::from_bits(crate::array::js_array_get_f64(keys, i).to_bits()),
            name,
        )
    })
}

fn refresh(proto: *mut ObjectHeader, shape: u32) -> Proof {
    let exec_index = if super::get_accessor_descriptor(proto as usize, "exec").is_none() {
        field_index(proto, b"exec")
    } else {
        None
    };
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

pub(crate) fn replace(value: f64) -> bool {
    if !exec(value) {
        return false;
    }
    let symbol = crate::symbol::well_known_symbol_if_cached("replace");
    if symbol.is_null() {
        return false;
    }
    let key = js_nanbox_pointer(symbol as i64);
    if unsafe { crate::symbol::js_object_has_own_symbol_property(value, key) } {
        return false;
    }
    PROOF.with(|cell| {
        let mut proof = cell.get();
        if !proof.flags {
            return false;
        }
        let epoch = crate::symbol::PERRY_SYMBOL_PROPERTY_IC_EPOCH.load(Ordering::Acquire);
        if proof.symbol_epoch != epoch {
            let proto = thunks::recorded_regexp_prototype();
            proof.replace =
                crate::symbol::symbol_accessor_descriptor_bits(proto as usize, symbol as usize)
                    .is_none()
                    && crate::symbol::symbol_property_root_bits(proto as usize, symbol as usize)
                        .is_some_and(|v| {
                            native(
                                f64::from_bits(v),
                                crate::regex::perex_replace::regexp_thunk as *const u8,
                            )
                        });
            proof.symbol_epoch = epoch;
            cell.set(proof);
        }
        proof.replace
    })
}
