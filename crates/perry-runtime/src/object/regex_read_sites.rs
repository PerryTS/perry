//! RegExp builtin Gets use the ordinary runtime read sites. No RegExp
//! shape, slot, prototype, accessor or epoch facts live outside those sites.
use super::field_get_set::runtime_read_site::{object_receiver, RuntimeReadSite};
use super::method_site::read_holder::probe::{Answer, Key};
use super::regex_proto_thunks as thunks;

crate::perry_thread_local! {
    // This RegExpExec site is primed only after the private matcher read
    // succeeds. A hit on that same receiver shape also carries the brand;
    // the general observable Get below uses its separate site.
    static EXEC_READ: RuntimeReadSite = const { RuntimeReadSite::new() };
    static READS: [RuntimeReadSite; 16] = const { [const { RuntimeReadSite::new() }; 16] };
    static SYMBOL_READS: [RuntimeReadSite; 7] = const { [const { RuntimeReadSite::new() }; 7] };
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
    READS.with(|sites| unsafe { sites[index].probe(obj, Key::Name(NAMES[index])) })
}

fn data_is(value: f64, index: usize, function: *const u8) -> bool {
    matches!(probe(value, index), Some(Answer::Data(bits)) if native(bits, function))
}

fn getter_is(value: f64, index: usize, function: *const u8) -> bool {
    let Some(obj) = object_receiver(value) else {
        return false;
    };
    READS.with(|sites| unsafe {
        sites[index]
            .probe_getter_code(obj, Key::Name(NAMES[index]))
            .is_some_and(|code| code == function as usize)
    })
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
    let data = crate::regex::regexp_data_of(value).filter(|_| {
        EXEC_READ.with(|site| unsafe {
            let object = (value.to_bits() & crate::value::POINTER_MASK) as *mut super::ObjectHeader;
            match site.read_leaf(object) {
                Some(method) => thunks::is_builtin_regexp_exec(method),
                None => matches!(site.probe(object, Key::Name(b"exec")), Some(Answer::Data(bits))
                    if thunks::is_builtin_regexp_exec(f64::from_bits(bits))),
            }
        })
    });
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
    let hit = data_is(value, 1, thunks::regex_proto_test_thunk as *const u8);
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

/// The eight observable Gets in the flags getter, in spec order. Every
/// accessor lane is checked even when its holder shape is unchanged.
pub(crate) fn flag_getters(value: f64) -> bool {
    let Some(obj) = object_receiver(value) else {
        return false;
    };
    READS.with(|sites| {
        [
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
        .all(|(i, function)| unsafe {
            sites[i + 3]
                .probe_getter_code(obj, Key::Name(NAMES[i + 3]))
                .is_some_and(|code| code == function as usize)
        })
    })
}

pub(crate) fn flags(value: f64) -> bool {
    getter_is(value, 2, thunks::regex_proto_flags_getter as *const u8) && flag_getters(value)
}

#[derive(Clone, Copy)]
pub(crate) enum Method {
    Replace,
    Match,
    Split,
}
impl Method {
    fn index(self) -> usize {
        self as usize
    }
    fn function(self) -> *const u8 {
        match self {
            Self::Replace => crate::regex::perex_replace::regexp_thunk as *const u8,
            Self::Match => crate::regex::perex_match_search::match_thunk as *const u8,
            Self::Split => crate::regex::perex_split::regexp_thunk as *const u8,
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
    SYMBOL_READS.with(|sites| unsafe {
        let site = &sites[index];
        if let Some(answer) = site.probe_leaf(obj) {
            return Some(answer);
        }
        let symbol = crate::symbol::well_known_symbol_if_cached(SYMBOLS[index]);
        if symbol.is_null() {
            return None;
        }
        site.probe(obj, Key::Symbol(symbol as usize))
    })
}

pub(crate) fn method(value: f64, method: Method) -> bool {
    crate::regex::regexp_data_of(value).is_some()
        && matches!(symbol_probe(value, method.index()), Some(Answer::Data(bits)) if native(bits, method.function()))
}

pub(crate) fn replace(value: f64) -> bool {
    method(value, Method::Replace) && exec_is_builtin(value) && flags(value)
}

pub(crate) fn split(value: f64) -> bool {
    // Inside the builtin @@split body, there is no further Get(@@split).
    // String dispatch already performed it; exec admission also carries brand.
    if !exec_is_builtin(value) || !flags(value) {
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
    Some(crate::regex::perex_api::caught(|| {
        READS.with(|sites| unsafe { sites[index].read(obj, NAMES[index]) })
    }))
}

pub(crate) fn read_symbol_data(owner: &crate::gc::RuntimeHandle<'_>, name: &str) -> Option<f64> {
    let index = SYMBOLS.iter().position(|n| *n == name)?;
    match symbol_probe(owner.get_nanbox_f64(), index)? {
        Answer::Data(bits) => Some(f64::from_bits(bits)),
        Answer::Getter(_) => None,
    }
}
