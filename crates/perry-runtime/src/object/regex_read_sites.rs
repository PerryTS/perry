//! RegExp builtin operations share the ordinary method-site shape proof.
//! Observable Gets retain the ordinary runtime read sites.
use super::field_get_set::runtime_read_site::{self as read, object_receiver, Answer, Key};
use super::regex_proto_thunks as thunks;
use std::cell::Cell;

crate::perry_thread_local! {
    // This RegExpExec site is primed only after the private matcher read
    // succeeds. A hit on that same receiver shape also carries the brand;
    // the general observable Get below uses its separate site.
    static BUILTIN: Cell<super::method_site::MethodSiteSlot> = const { Cell::new(std::ptr::null_mut()) };
}

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
    // The aggregate shape proof already covers exec. Share it with replace
    // and test instead of probing a second holder/getter site on pristine
    // receivers. Mutated receivers retain the narrower observable-exec proof.
    let data = if builtin_behavior(value) {
        crate::regex::regexp_data_of(value)
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

/// The complete builtin receiver proof. Flags are read from internal data
/// only after the method-site receiver and holder words still match.
#[inline]
pub(crate) fn builtin_behavior(value: f64) -> bool {
    let Some(object) = object_receiver(value) else {
        return false;
    };
    BUILTIN.with(|site| unsafe {
        super::method_site::builtin_shape_proof(site.as_ptr(), object, |object| {
            prove_builtin_behavior(object)
        })
    })
}

#[cold]
unsafe fn prove_builtin_behavior(
    object: *const super::ObjectHeader,
) -> Option<*const super::ObjectHeader> {
    use super::shapes;
    if crate::agent::current_agent() != crate::agent::PRIMARY_AGENT
        || super::field_get_set::accessor_receiver_override_armed()
        || super::prototype_chain::resolution_stack_depth() != 0
    {
        return None;
    }
    let meta = (*object).meta;
    if !meta.is_null()
        && ((*meta).elements != 0
            || (*meta).flags
                & (super::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER
                    | super::OBJECT_META_FLAG_NATIVE_ALIAS)
                != 0)
    {
        return None;
    }
    let receiver = shapes::object_shape_descriptor(object)?;
    if !receiver.object_kind.is_ordinary_layout()
        || !shapes::is_site_matchable_shape_id(shapes::object_shape_stamp(object))
        || receiver.hole_count != 0
    {
        return None;
    }
    let value = crate::value::js_nanbox_pointer(object as i64);
    crate::regex::regexp_data_of(value)?;
    let prototype = crate::regex::intrinsic_prototype();
    if receiver.record_ref()?.prototype_word() != prototype.to_bits() {
        return None;
    }
    let holder = object_receiver(prototype)?;
    let shape = shapes::object_shape_descriptor(holder)?;
    if !shape.object_kind.is_ordinary_layout()
        || !shapes::is_site_matchable_shape_id(shapes::object_shape_stamp(holder))
        || shape.hole_count != 0
    {
        return None;
    }

    // Named absence is immutable receiver shape metadata. Inspect getter
    // identities on this exact holder only while priming; replacing a pair
    // retires its shape through the ordinary descriptor transition.
    let keys = receiver.keys as usize as *const crate::array::ArrayHeader;
    for name in &NAMES[..11] {
        if super::keys_find_slot_by_bytes_resolved(keys, receiver.logical_key_count, name).is_some()
        {
            return None;
        }
    }
    if !data_is(prototype, 0, thunks::regex_proto_exec_thunk as *const u8) {
        return None;
    }
    let exec = super::keys_find_slot_by_bytes_resolved(
        shape.keys as usize as *const crate::array::ArrayHeader,
        shape.logical_key_count,
        b"exec",
    )?;
    if !constfn_code(&shape, exec, thunks::regex_proto_exec_thunk as *const u8) {
        return None;
    }
    let test = super::keys_find_slot_by_bytes_resolved(
        shape.keys as usize as *const crate::array::ArrayHeader,
        shape.logical_key_count,
        b"test",
    )?;
    if !constfn_code(&shape, test, thunks::regex_proto_test_thunk as *const u8) {
        return None;
    }
    for (index, code) in [
        thunks::regex_proto_flags_getter as *const u8,
        thunks::regex_proto_has_indices_getter as *const u8,
        thunks::regex_proto_global_getter as *const u8,
        thunks::regex_proto_ignore_case_getter as *const u8,
        thunks::regex_proto_multiline_getter as *const u8,
        thunks::regex_proto_dot_all_getter as *const u8,
        thunks::regex_proto_unicode_getter as *const u8,
        thunks::regex_proto_unicode_sets_getter as *const u8,
        thunks::regex_proto_sticky_getter as *const u8,
    ]
    .into_iter()
    .enumerate()
    {
        if !read::getter_code(holder, Key::Name(NAMES[index + 2]))
            .is_some_and(|actual| actual == code as usize)
        {
            return None;
        }
    }
    for method in [
        Method::Replace,
        Method::Match,
        Method::Split,
        Method::Search,
        Method::MatchAll,
    ] {
        let symbol = crate::symbol::well_known_symbol_if_cached(SYMBOLS[method.index()]);
        if symbol.is_null() || super::shaped_symbols::position(object, symbol as usize).is_some() {
            return None;
        }
        let slot = super::shaped_symbols::position(holder, symbol as usize)?;
        if !constfn_code(&shape, slot, method.function()) {
            return None;
        }
    }
    Some(holder)
}

// `position` is the key's position in the holder's key list.
unsafe fn constfn_code(
    shape: &super::shapes::ShapeDescriptor,
    position: u32,
    code: *const u8,
) -> bool {
    let slot = super::key_attrs::key_storage_slot(
        shape.keys as usize as *const crate::array::ArrayHeader,
        position,
    );
    slot < super::field_rep::REP_SLOTS
        && shape.special_constfn_mask & (1 << slot) != 0
        && shape.constfn_infos().iter().any(|entry| {
            u32::from(entry.slot) == slot
                && (*(entry.info as usize as *const crate::closure::JsFunctionInfo)).code == code
        })
}

pub(crate) fn flag_getters(value: f64) -> bool {
    builtin_behavior(value)
}
pub(crate) fn flags(value: f64) -> bool {
    builtin_behavior(value)
}

#[derive(Clone, Copy)]
pub(crate) enum Method {
    Replace,
    Match,
    Split,
    Search,
    MatchAll,
}
impl Method {
    fn index(self) -> usize {
        match self {
            Self::Replace => 0,
            Self::Match => 1,
            Self::Split => 2,
            Self::Search => 4,
            Self::MatchAll => 5,
        }
    }
    fn function(self) -> *const u8 {
        match self {
            Self::Replace => crate::regex::perex_replace::regexp_thunk as *const u8,
            Self::Match => crate::regex::perex_match_search::match_thunk as *const u8,
            Self::Split => crate::regex::perex_split::regexp_thunk as *const u8,
            Self::Search => crate::regex::perex_match_search::search_thunk as *const u8,
            Self::MatchAll => crate::regex::match_all::regexp_thunk as *const u8,
        }
    }
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

pub(crate) fn method(value: f64, _method: Method) -> bool {
    builtin_behavior(value)
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
