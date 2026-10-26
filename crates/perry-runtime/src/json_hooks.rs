//! JSON-owned arms of generic array, GC and IPC paths.
use crate::feature_hooks::Hook;
use crate::json_tape::LazyArrayHeader;
use crate::{JSValue, StringHeader};

static FORCE: Hook<unsafe fn(*mut LazyArrayHeader) -> *mut crate::array::ArrayHeader> =
    Hook::empty();
static COLD_GET: Hook<unsafe fn(*mut LazyArrayHeader, u32) -> JSValue> = Hook::empty();

pub unsafe fn force_materialize_lazy(
    array: *mut LazyArrayHeader,
) -> *mut crate::array::ArrayHeader {
    FORCE
        .get()
        .map_or_else(std::ptr::null_mut, |materialize| materialize(array))
}
pub(crate) unsafe fn lazy_get_rooted(array: *mut LazyArrayHeader, index: u32) -> JSValue {
    COLD_GET
        .get()
        .map_or_else(JSValue::undefined, |get| get(array, index))
}
static PARSE_RESULT: Hook<unsafe fn(*const StringHeader) -> Result<JSValue, f64>> = Hook::empty();
static PARSE: Hook<unsafe extern "C" fn(*const StringHeader) -> JSValue> = Hook::empty();
static NOTE_CARRIERS: Hook<fn()> = Hook::empty();
static UNMATERIALIZED: Hook<unsafe fn(usize) -> bool> = Hook::empty();
static NOTE_MATERIALIZED: Hook<fn()> = Hook::empty();
static AFTER_COLD_READ: Hook<unsafe fn(*mut LazyArrayHeader, bool, bool)> = Hook::empty();

pub(crate) unsafe fn parse_result(text: *const StringHeader) -> Result<JSValue, f64> {
    PARSE_RESULT.get().map_or_else(
        || Err(f64::from_bits(crate::value::TAG_UNDEFINED)),
        |parse| parse(text),
    )
}
pub(crate) unsafe fn parse(text: *const StringHeader) -> JSValue {
    PARSE
        .get()
        .map_or_else(JSValue::undefined, |parse| parse(text))
}
pub(crate) fn note_carriers() {
    if let Some(note) = NOTE_CARRIERS.get() {
        note();
    }
}
pub(crate) unsafe fn unmaterialized(address: usize) -> bool {
    UNMATERIALIZED.get().is_some_and(|probe| probe(address))
}
pub(crate) fn note_materialized() {
    if let Some(note) = NOTE_MATERIALIZED.get() {
        note();
    }
}
pub(crate) unsafe fn after_cold_read(array: *mut LazyArrayHeader, flip: bool, completed: bool) {
    if let Some(note) = AFTER_COLD_READ.get() {
        note(array, flip, completed);
    }
}
pub(crate) fn install() {
    FORCE.set(crate::json_tape::force_materialize_lazy_impl);
    COLD_GET.set(crate::json_tape::lazy_get_rooted_impl);
    PARSE.set(crate::json::js_json_parse);
    PARSE_RESULT.set(crate::json::js_json_parse_result);
    NOTE_CARRIERS.set(crate::json::note_parse_shape_cache_carriers);
    UNMATERIALIZED.set(crate::json::traversal_feedback::lazy_array_unmaterialized);
    NOTE_MATERIALIZED.set(crate::json::traversal_feedback::note_compiled_materialization);
    AFTER_COLD_READ.set(crate::json::traversal_feedback::after_cold_read);
}
pub(crate) fn register_roots() {
    // Scanner registration is per thread; installers run on every gc_init.
    crate::gc::gc_register_mutable_root_scanner_named(
        "json_parse_mutable_root_scanner",
        crate::gc::json_parse_mutable_root_scanner,
    );
    crate::gc::gc_register_mutable_root_scanner_named(
        "crate::json::raw_json::scan_raw_json_key_root_mut",
        crate::json::raw_json::scan_raw_json_key_root_mut,
    );
}
