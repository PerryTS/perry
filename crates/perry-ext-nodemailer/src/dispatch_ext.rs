//! Handle-dispatch extension for the `nodemailer` transporter.
//!
//! ## Why this exists
//!
//! `nodemailer.createTransport(...)` is a module-level call, so codegen lowers
//! it through the native table and it works. Its RESULT, though, is a plain
//! handle id returned as `NR_F64` — a number — and there is no TypeScript class
//! behind it, so `transporter.sendMail(...)` and `transporter.verify()` are
//! lowered as generic method calls on an untyped receiver. Those route through
//! the runtime's `HANDLE_METHOD_DISPATCH` slow path, and **no arm anywhere
//! claimed a nodemailer handle**: every call failed with
//! `TypeError: (number).sendMail is not a function`.
//!
//! That is not a P6 regression — it reproduces on the base commit, in every
//! call shape (module scope, inside an `async fn`, `.then` chain), for both
//! methods. It is recorded in `docs/turnloop/p6-report.md` under the defects
//! this phase found. It is fixed here rather than merely reported because
//! without it the whole SMTP surface is unreachable from JS and "SMTP now runs
//! on turnloop" would be a claim nothing could check.
//!
//! ## The gate
//!
//! The extension claims a name only when the handle is one of THIS crate's
//! transporters and the name is one of the two native methods. Anything else
//! returns "not claimed" so the runtime falls through to the prototype chain —
//! the rule `perry-ext-http`'s extension documents, and for the same reason: a
//! user object wrapping the transporter must keep its own methods.

use std::sync::Once;

use perry_ffi::{get_handle, Handle, JsValue, Promise};

use crate::SmtpTransportHandle;

unsafe extern "C" {
    fn js_register_handle_method_dispatch_extension(
        f: unsafe extern "C" fn(i64, *const u8, usize, *const f64, usize, *mut f64) -> i32,
    );
}

/// Idempotent; called from `js_nodemailer_create_transport`, so it runs before
/// any method call on a transporter can be made.
pub(crate) fn ensure_registered() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        // SAFETY: a plain registration with a `'static` function pointer.
        unsafe { js_register_handle_method_dispatch_extension(transporter_method_dispatch_ext) };
    });
}

/// # Safety
/// Called by the runtime's composite dispatcher with a live name and argument
/// slice, and an `out` pointer for one `f64`.
unsafe extern "C" fn transporter_method_dispatch_ext(
    handle: i64,
    name_ptr: *const u8,
    name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
    out: *mut f64,
) -> i32 {
    if handle <= 0 || name_ptr.is_null() || name_len == 0 || out.is_null() {
        return 0;
    }
    // Registry membership FIRST: an id that is not one of ours must never be
    // claimed, whatever the method name is (native handle id spaces are not
    // unified, #91).
    if get_handle::<SmtpTransportHandle>(handle as Handle).is_none() {
        return 0;
    }
    // SAFETY: the caller's contract; borrowed for this call only.
    let name = unsafe { std::slice::from_raw_parts(name_ptr, name_len) };
    let Ok(name) = std::str::from_utf8(name) else {
        return 0;
    };
    let args: &[f64] = if args_len == 0 || args_ptr.is_null() {
        &[]
    } else {
        // SAFETY: as above.
        unsafe { std::slice::from_raw_parts(args_ptr, args_len) }
    };
    let promise: *mut Promise = match name {
        "sendMail" => {
            let options = args
                .first()
                .copied()
                .unwrap_or_else(|| f64::from_bits(JsValue::UNDEFINED.bits()));
            // SAFETY: the entry point's own contract — a handle from this
            // registry and a NaN-boxed options value.
            unsafe { crate::js_nodemailer_send_mail(handle as Handle, options) }
        }
        "verify" => crate::js_nodemailer_verify(handle as Handle),
        // `close`, `isIdle`, `use`, … are nodemailer API this binding does not
        // implement; not claiming them keeps the existing "not a function"
        // error rather than silently answering `undefined`.
        _ => return 0,
    };
    // A Promise reaches JS as a NaN-boxed POINTER, the same encoding the
    // native table's `NR_GCPTR` return produces for the statically typed call.
    // SAFETY: `out` is the caller's one-`f64` slot.
    unsafe { *out = f64::from_bits(JsValue::from_object_ptr(promise).bits()) };
    1
}
