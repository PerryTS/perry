//! RegExp builtin operations prove their receiver from the RegExp birth
//! layout and the prototype's shadow bit (`regex::instance::builtin`).
//! Observable Gets retain the ordinary runtime reads.
use super::field_get_set::runtime_read_site::{self as read, object_receiver, Answer, Key};
use super::regex_proto_thunks as thunks;

const NAMES: [&[u8]; 16] = [
    b"exec",
    b"test",
    b"flags",
    b"hasIndices",
    b"global",
    b"ignoreCase",
    b"multiline",
    b"dotAll",
    b"unicode",
    b"unicodeSets",
    b"sticky",
    b"constructor",
    b"lastIndex",
    b"source",
    b"index",
    b"length",
];

fn native(bits: u64, function: *const u8) -> bool {
    let value = f64::from_bits(bits);
    crate::value::JSValue::from_bits(bits).is_pointer()
        && crate::closure::get_valid_func_ptr(
            crate::value::js_nanbox_get_pointer(value) as *const crate::closure::ClosureHeader
        ) == function
}

fn probe(value: f64, index: usize) -> Option<Answer> {
    let obj = object_receiver(value)?;
    unsafe { read::probe(obj, Key::Name(NAMES[index])) }
}

fn data_is(value: f64, index: usize, function: *const u8) -> bool {
    matches!(probe(value, index), Some(Answer::Data(bits)) if native(bits, function))
}

#[inline]
pub(crate) fn exec_is_builtin(value: f64) -> bool {
    builtin_exec_data(value).is_some()
}

/// `Get(R, "exec")` proven to be the builtin without running anything: the
/// RegExp's immutable data, or `None` for any receiver that needs the
/// observable path.
#[inline]
pub(crate) fn builtin_exec_data(value: f64) -> Option<*const crate::regex::RegExpData> {
    // The brand first: the private matcher read is one shape compare, and its
    // answer proves a live, ordinary, branded receiver, which is all the exec
    // site's read needs to know about it. Every other value is not a RegExp
    // and keeps the observable path.
    // The builtin fact already covers exec and hands over the matcher slot.
    // Mutated receivers retain the narrower observable-exec proof.
    let data = if let Some(re) = crate::regex::instance::builtin(value) {
        // SAFETY: admitted just now; nothing collected since.
        Some(unsafe { crate::regex::instance::builtin_data(re) })
    } else {
        crate::regex::regexp_data_of(value).filter(|_| {
            let object = (value.to_bits() & crate::value::POINTER_MASK) as *mut super::ObjectHeader;
            matches!(unsafe { read::probe(object, Key::Name(b"exec")) },
                Some(Answer::Data(bits))
                if thunks::is_builtin_regexp_exec(f64::from_bits(bits)))
        })
    };
    if crate::hot_diag::regex_on() {
        crate::hot_diag::regex_counters(|d| {
            if data.is_some() {
                d.proof_exec_hit += 1;
            } else {
                d.proof_exec_miss += 1;
            }
        });
    }
    data
}

pub(crate) fn test(value: f64) -> bool {
    let hit =
        builtin_behavior(value) || data_is(value, 1, thunks::regex_proto_test_thunk as *const u8);
    if crate::hot_diag::regex_on() {
        crate::hot_diag::regex_counters(|d| {
            if hit {
                d.proto_test_hit += 1;
            } else {
                d.proto_test_miss += 1;
            }
        });
    }
    hit
}

/// The complete builtin receiver proof: the RegExp birth-layout fact
/// ([`crate::regex::instance::builtin`]), one header word and the
/// prototype's shadow bit, both set where a change happens. Flags are then
/// read from internal data.
#[inline]
pub(crate) fn builtin_behavior(value: f64) -> bool {
    crate::regex::instance::builtin(value).is_some()
}

pub(crate) fn flag_getters(value: f64) -> bool {
    builtin_behavior(value)
}

const SYMBOLS: [&str; 7] = [
    "replace",
    "match",
    "split",
    "species",
    "search",
    "matchAll",
    "toPrimitive",
];

fn symbol_probe(value: f64, index: usize) -> Option<Answer> {
    let obj = object_receiver(value).or_else(|| unsafe {
        // Functions' symbol properties are already ordinary shaped bags.
        super::shaped_symbols::owner(crate::value::js_nanbox_get_pointer(value) as usize)
    })?;
    let symbol = crate::symbol::well_known_symbol_if_cached(SYMBOLS[index]);
    if symbol.is_null() {
        return None;
    }
    unsafe { read::probe(obj, Key::Symbol(symbol as usize)) }
}

pub(crate) fn split(value: f64) -> bool {
    // Inside the builtin @@split body, there is no further Get(@@split).
    // String dispatch already performed it; exec admission also carries brand.
    if !builtin_behavior(value) {
        return false;
    }
    let Some(Answer::Data(constructor)) = probe(value, 11) else {
        return false;
    };
    thunks::is_intrinsic_regexp_constructor(f64::from_bits(constructor))
        && matches!(symbol_probe(f64::from_bits(constructor), 3), Some(Answer::Getter(bits))
            if native(bits, super::global_this::builtin_species_getter_thunk as *const u8))
}

/// Observable Get, with exactly one invocation of an overridden accessor.
pub(crate) fn read_named(
    owner: &crate::gc::RuntimeHandle<'_>,
    name: &[u8],
) -> Option<Result<f64, crate::regex::perex_runtime::EngineError>> {
    let index = NAMES.iter().position(|n| *n == name)?;
    let obj = object_receiver(owner.get_nanbox_f64())?;
    Some(crate::regex::perex_api::caught(|| unsafe {
        read::get(obj, NAMES[index])
    }))
}

pub(crate) fn read_symbol_data(owner: &crate::gc::RuntimeHandle<'_>, name: &str) -> Option<f64> {
    let index = SYMBOLS.iter().position(|n| *n == name)?;
    match symbol_probe(owner.get_nanbox_f64(), index)? {
        Answer::Data(bits) => Some(f64::from_bits(bits)),
        Answer::Getter(_) => None,
    }
}

#[cfg(test)]
#[path = "regex_builtin_shape_tests.rs"]
mod tests;
