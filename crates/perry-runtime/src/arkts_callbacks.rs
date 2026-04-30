//! HarmonyOS callback registry for ArkUI → Perry NAPI bridge (Phase 2 v2).
//!
//! ArkUI renders pages emitted by `perry-codegen-arkts`. When the user
//! authors `Button("Save", () => { count++ })` in TypeScript, the harvest
//! pass:
//!
//! 1. Captures the closure expression, assigns it slot `idx`
//! 2. Emits `Button('Save').onClick(() => perryEntry.invokeCallback(<idx>))`
//!    in the .ets
//! 3. Injects a `perry_arkts_register_callback(<idx>, <closure>)` call
//!    into Perry's `main()` so the closure pointer ends up in this table
//!
//! On `main()` startup the closures get registered. When the user later
//! taps Save in ArkUI, the .ets `onClick` fires `perryEntry.invokeCallback(0)`
//! through NAPI; that lands in `perry_arkts_invoke_callback` here, which
//! looks up slot 0, unboxes the closure pointer, and calls
//! `js_closure_call0` — running the original Perry TS closure body.
//!
//! Phase 2 v2 only supports 0-arg closures (Button.onClick). Toggle's
//! `(isOn: boolean) => ...`, TextField's `(value: string) => ...`, and
//! Slider's `(value: number) => ...` need NaN-box marshaling for the
//! arg and are deferred to v2.5.
//!
//! GC: registered closure pointers are scanned via
//! `arkts_callbacks_root_scanner`, registered in `gc_init`, so the
//! generational mark-sweep doesn't reclaim them between callbacks.

use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_uint};
use std::sync::Mutex;

use crate::closure::{js_closure_call0, ClosureHeader};
use crate::value::{POINTER_MASK, TAG_UNDEFINED};

// POINTER_TAG is private to the value module; redeclare the constant here
// so we can match against it. Must stay in sync with value.rs.
const POINTER_TAG_BITS: u64 = 0x7FFD_0000_0000_0000;

// OpenHarmony hilog NDK. The harmonyos link line passes -lhilog_ndk.z so
// this resolves at .so load time. Calling with a literal fmt + no
// variadic args is well-defined under C99 — the variadic save area
// stays untouched. We pre-format messages in Rust and pass them in
// `fmt`; tag is a fixed string that surfaces in DevEco's hilog filter.
extern "C" {
    fn OH_LOG_Print(
        log_type: c_int,
        level: c_int,
        domain: c_uint,
        tag: *const c_char,
        fmt: *const c_char,
    ) -> c_int;
}

const HILOG_TYPE_APP: c_int = 0;
// LOG_ERROR (6) — high enough that hilog won't filter it under any common
// runlevel. Using INFO (4) on the OH emulator was producing zero output
// even though `A00000/perry` info-level lines from ArkTS's console.info
// surfaced — the level threshold differs by domain.
const HILOG_LEVEL_ERROR: c_int = 6;
// Domain 0 matches what ArkTS's `console.log` uses (visible as `A00000/<tag>`).
// LOG_APP requires domain to fit in 16 bits; the previous 0xC0FFEE failed
// validation silently. Tag distinguishes Perry's lines from app lines.
const HILOG_DOMAIN_APP: c_uint = 0x0000;

fn arkts_log(msg: &str) {
    let Ok(c) = CString::new(msg) else { return };
    let tag = c"perry-arkts".as_ptr();
    unsafe {
        OH_LOG_Print(
            HILOG_TYPE_APP,
            HILOG_LEVEL_ERROR,
            HILOG_DOMAIN_APP,
            tag,
            c.as_ptr(),
        );
    }
}

/// Sibling-module helper for `ohos_napi::invoke_callback` to emit
/// hilog entries via the same wiring without re-exporting the extern.
pub(crate) fn arkts_log_napi(msg: &str) {
    arkts_log(msg);
}

/// Route Perry's `console.log` family from stdout (which has no terminal
/// when the .so is loaded by ArkTS) to hilog. Used by the module-scoped
/// `println!` override in `builtins.rs`. Tag distinguishes Perry-emitted
/// log lines from ArkTS-emitted ones in DevEco/hdc.
pub fn ohos_stdout_println(msg: &str) {
    let Ok(c) = CString::new(msg) else { return };
    let tag = c"perry".as_ptr();
    unsafe {
        OH_LOG_Print(
            HILOG_TYPE_APP,
            // INFO matches Node's console.log default level. The
            // diagnostic arkts_log uses ERROR specifically because the
            // INFO threshold can be filtered out per-domain on some OHOS
            // emulator configs; for user-facing console.log output we
            // should still emit at INFO since that's the canonical
            // mapping (and the user's hilog filter will pick it up).
            4, // LOG_INFO
            HILOG_DOMAIN_APP,
            tag,
            c.as_ptr(),
        );
    }
}

static CALLBACKS: Mutex<Vec<f64>> = Mutex::new(Vec::new());

/// Register a Perry closure (NaN-boxed f64) at the given slot. Slots
/// beyond the current Vec length are filled with TAG_UNDEFINED so the
/// caller can register slots in any order.
#[no_mangle]
pub extern "C" fn perry_arkts_register_callback(idx: i64, closure_d: f64) {
    let mut cbs = CALLBACKS.lock().unwrap();
    let i = idx as usize;
    while cbs.len() <= i {
        cbs.push(f64::from_bits(TAG_UNDEFINED));
    }
    cbs[i] = closure_d;
    arkts_log(&format!(
        "register slot={} closure_bits=0x{:016x}",
        i,
        closure_d.to_bits()
    ));
}

/// Invoke a registered closure by slot. Returns NaN-boxed `undefined` if
/// the slot is out of range, never registered, or holds a non-pointer
/// value (defensive — should never happen with codegen-emitted shape).
#[no_mangle]
pub extern "C" fn perry_arkts_invoke_callback(idx: i64) -> f64 {
    arkts_log(&format!("invoke ENTER idx={}", idx));
    // Snapshot under lock then drop so the closure body can re-enter
    // (e.g. a button handler that itself registers another callback).
    let closure_d = {
        let cbs = CALLBACKS.lock().unwrap();
        let i = idx as usize;
        if i >= cbs.len() {
            arkts_log(&format!(
                "invoke OOB idx={} cbs.len={}",
                i,
                cbs.len()
            ));
            return f64::from_bits(TAG_UNDEFINED);
        }
        cbs[i]
    };
    let bits = closure_d.to_bits();
    arkts_log(&format!(
        "invoke idx={} closure_bits=0x{:016x}",
        idx, bits
    ));
    if (bits & !POINTER_MASK) != POINTER_TAG_BITS {
        arkts_log(&format!("invoke not-a-pointer idx={}", idx));
        return f64::from_bits(TAG_UNDEFINED);
    }
    let raw = (bits & POINTER_MASK) as *const ClosureHeader;
    if raw.is_null() {
        arkts_log(&format!("invoke null-pointer idx={}", idx));
        return f64::from_bits(TAG_UNDEFINED);
    }
    arkts_log(&format!("invoke calling closure idx={}", idx));
    let result = js_closure_call0(raw);
    arkts_log(&format!("invoke RETURN idx={}", idx));
    result
}

/// GC root scanner. Marks each registered closure pointer as live so the
/// generational mark-sweep doesn't reclaim closure bodies between taps.
pub fn arkts_callbacks_root_scanner(mark: &mut dyn FnMut(f64)) {
    if let Ok(cbs) = CALLBACKS.try_lock() {
        for &c in cbs.iter() {
            mark(c);
        }
    }
}
