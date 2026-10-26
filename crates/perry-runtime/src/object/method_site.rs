//! What remains of the method-call site memo after object model step 5,
//! phase 2 (`OBJECT_MODEL_2026-10-13.md` §3.7): a method call is a probe
//! [[Get]] and a call (`method_call.rs`), and nothing here is per site.
//!
//! * [`read_holder`]: the state-free shape walk the probe falls back to for
//!   builtin prototypes it declines;
//! * [`proto_constant`]: object model step 3's prototype-constant contract
//!   witnesses (`PERRY_CHECK_CONTRACTS` builds);
//! * the process-wide worker gate ([`WORKER_AGENTS_EXIST`]).
use crate::object::ObjectHeader;
use std::sync::atomic::Ordering;

mod proto_constant;
pub use proto_constant::{
    js_proto_constant_contract_check, js_proto_holder_contract_check,
    js_proto_shadow_contract_check,
};
pub(crate) mod read_holder;

/// Sticky process-wide gate: set before the first worker agent runs any
/// code. Code that may hand a primary-heap object to another agent reads it.
#[cfg_attr(not(test), export_name = "PERRY_METHOD_SITE_WORKERS_PRESENT")]
pub(crate) static WORKER_AGENTS_EXIST: std::sync::atomic::AtomicU8 =
    std::sync::atomic::AtomicU8::new(0);

/// Run a gate-sensitive unit in a fresh test process. Worker startup is
/// process-wide and sticky: clearing it in a parallel libtest process can
/// re-enable a worker's access to primary-heap holder pointers.
#[cfg(test)]
pub(crate) fn run_with_fresh_worker_gate(filter: &str) -> bool {
    const MARKER: &str = "PERRY_A2_FRESH_WORKER_GATE_TEST";
    if std::env::var_os(MARKER).as_deref() == Some(std::ffi::OsStr::new(filter)) {
        return true;
    }
    let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .arg("--test-threads=1")
        .arg(filter)
        .env(MARKER, filter)
        .output()
        .expect("run filtered test in a fresh process");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains("running 1 test") && stdout.contains("1 passed"),
        "isolated test {filter} failed or matched no test:\n{stdout}\n{stderr}",
    );
    false
}

/// Called by `agent::enter_worker_agent` before the worker runs any code.
pub fn note_worker_agent() {
    WORKER_AGENTS_EXIST.store(1, Ordering::SeqCst);
}

/// Is `PERRY_METHOD_SITE_STATS` set? Read once; the reader that settles the
/// answer installs the exit report (the dynamic-key read census).
pub(crate) fn stats_report_enabled() -> bool {
    per_test_global! {
        static ON: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
    }
    match ON.load(Ordering::Relaxed) {
        0 => {}
        state => return state == 2,
    }
    let on = std::env::var_os("PERRY_METHOD_SITE_STATS").is_some();
    let state = if on { 2 } else { 1 };
    if ON
        .compare_exchange(0, state, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
        && on
    {
        extern "C" fn report() {
            super::dynamic_key_read::census::report();
        }
        unsafe { libc::atexit(report) };
    }
    on
}

/// The emitted fused receiver test: a POINTER-tagged value whose payload is
/// above the native-handle band, i.e. a heap object.
#[inline(always)]
pub(crate) fn is_site_receiver(recv: f64) -> bool {
    const BIAS: u64 = crate::value::POINTER_TAG | perry_abi::RECEIVER_HANDLE_FLOOR as u64;
    const SPAN: u64 = (1u64 << 48) - perry_abi::RECEIVER_HANDLE_FLOOR as u64;
    recv.to_bits().wrapping_sub(BIAS) < SPAN
}

fn name_refused(name: &[u8]) -> bool {
    name.is_empty()
        || name[0] == b'#'
        || name.starts_with(b"__perry_")
        || name.starts_with(b"@@")
        || name == b"constructor"
}

/// The receiver, if it is an ordinary object a site may learn.
unsafe fn ordinary_receiver(addr: usize) -> Option<*const ObjectHeader> {
    if !crate::value::addr_class::is_above_handle_band(addr) {
        return None;
    }
    let header = crate::value::addr_class::try_read_gc_header(addr)?;
    // An OBJECT header is not a closure (closures carry GC_TYPE_CLOSURE).
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || !address_is_prime_stable(addr)
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || header._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0
    {
        return None;
    }
    let obj = addr as *const ObjectHeader;
    // `object_is_regular` without its second header read: the record names
    // the kind, and its summary says whether the layout can have holes, whose
    // optional keys are own or absent per object (`key_view`): no site
    // learns from such a receiver.
    let Some(record) = super::shapes::shape_record_of_receiver((*obj).shape_word()) else {
        return None;
    };
    if !record.object_kind().is_ordinary_layout()
        || super::key_view::summary_may_have_holes(record.summary())
        || super::overflow::has_overflow(obj)
        || (*obj).class_id == super::native_module::NATIVE_MODULE_CLASS_ID
        || super::class_registry::is_class_object_ptr(obj.cast())
        || crate::array::object_prototype_addr_matches(addr)
        || ((*obj).class_id == 0 && crate::url::is_url_object_shape(obj as *mut ObjectHeader))
    {
        return None;
    }
    let stamp = super::shapes::object_shape_stamp(obj);
    if !super::shapes::is_layout_word(stamp) {
        return None;
    }
    let meta = (*obj).meta;
    if !meta.is_null()
        && ((*meta).elements != 0
            || (*meta).flags & super::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER != 0)
    {
        return None;
    }
    Some(obj)
}

fn address_is_prime_stable(addr: usize) -> bool {
    crate::value::addr_class::is_plausible_heap_addr(addr)
        && crate::arena::classify_heap_generation(addr) != crate::arena::HeapGeneration::Unknown
}

/// The next prototype the way the inherited-read walk resolves it: the meta
/// record's `[[Prototype]]`, else a synthetic class's (`Object.create`, an ES5
/// constructor) registered prototype. Null for a default builtin prototype.
unsafe fn next_prototype(obj: *const ObjectHeader) -> *const ObjectHeader {
    let recorded = crate::object::shapes::object_prototype_word(obj);
    if recorded != 0 {
        let p = crate::value::JSValue::from_bits(recorded);
        if !p.is_pointer() {
            return std::ptr::null();
        }
        return p.as_pointer();
    }
    let class_id = (*obj).class_id;
    let synthetic = class_id >= 0x8000_0000
        && class_id < super::NEXT_SYNTHETIC_CLASS_ID.load(std::sync::atomic::Ordering::Relaxed);
    if !synthetic || !super::class_decl_prototype_object(class_id).is_null() {
        return std::ptr::null();
    }
    super::class_prototype_object(class_id)
}
