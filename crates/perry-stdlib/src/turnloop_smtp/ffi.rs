//! The C seam a separately linked binding uses to reach this engine.
//!
//! `perry-ext-nodemailer` is a `staticlib` with no Cargo edge to perry-stdlib,
//! so it cannot call [`super::send`] directly — the same constraint P1 solved
//! for `net` with `turnloop_net::abi`. It reaches these `#[no_mangle]` symbols
//! instead, which is the shape `perry-ext-undici` already uses to reach
//! `js_fetch_set_global_proxy`.
//!
//! The request is built with setters rather than passed as one `repr(C)`
//! struct: a struct would have to keep its layout in step across two crates
//! that are compiled separately, and the failure mode of a drift is reading a
//! length out of a pointer field. A builder has no layout to drift. Only the
//! *result* is a struct, and it is borrowed for the duration of the callback.
//!
//! Every string is `(pointer, length)` UTF-8, never NUL-terminated, and is
//! copied before the setter returns.

use std::cell::RefCell;
use std::collections::HashMap;

use super::{MailJob, Outcome, Sink, SmtpConfig};

/// One `(pointer, length)` string, as a binding reads it back.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PerrySmtpSlice {
    pub ptr: *const u8,
    pub len: usize,
}

impl PerrySmtpSlice {
    fn of(s: &str) -> Self {
        Self {
            ptr: s.as_ptr(),
            len: s.len(),
        }
    }
}

/// The outcome of one exchange. Borrowed for the duration of the callback; a
/// binding that needs any of it past that must copy.
#[repr(C)]
pub struct PerrySmtpResult {
    /// 1 on success, 0 on failure.
    pub ok: i32,
    /// The SMTP reply code, or -1 when the failure never reached one.
    pub response_code: i32,
    /// nodemailer's `err.code` on failure; empty on success.
    pub code: PerrySmtpSlice,
    /// The error message on failure, or the server's final reply on success.
    pub message: PerrySmtpSlice,
    /// The raw server response text.
    pub response: PerrySmtpSlice,
    pub message_id: PerrySmtpSlice,
    pub accepted: *const PerrySmtpSlice,
    pub accepted_len: usize,
    pub rejected: *const PerrySmtpSlice,
    pub rejected_len: usize,
}

/// Called once per accepted submission, on the owning thread.
pub type PerrySmtpDone = extern "C" fn(ctx: usize, result: *const PerrySmtpResult);

#[derive(Default)]
struct Draft {
    config: SmtpConfig,
    from: String,
    to: Vec<String>,
    message_id: String,
    message: Vec<u8>,
}

impl Default for SmtpConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 587,
            implicit_tls: false,
            require_tls: false,
            user: None,
            pass: None,
            client_name: "[127.0.0.1]".to_string(),
        }
    }
}

thread_local! {
    static DRAFTS: RefCell<HashMap<i64, Draft>> = RefCell::new(HashMap::new());
    static NEXT_DRAFT: RefCell<i64> = const { RefCell::new(1) };
    /// The binding's completion, per accepted submission. Keyed by the ctx the
    /// binding chose, so nothing here has to be a JS value.
    static CALLBACKS: RefCell<HashMap<usize, PerrySmtpDone>> = RefCell::new(HashMap::new());
}

/// # Safety
/// `ptr` must be null or point at `len` readable bytes.
unsafe fn text(ptr: *const u8, len: usize) -> String {
    if ptr.is_null() || len == 0 {
        return String::new();
    }
    // SAFETY: the caller's contract, and the bytes are copied here.
    String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(ptr, len) }).into_owned()
}

/// Whether this thread can take the turnloop SMTP path at all. A binding must
/// check this before building a draft; `false` means keep the existing
/// transport.
#[no_mangle]
pub extern "C" fn js_perry_smtp_available() -> i32 {
    i32::from(perry_runtime::turnloop_net::available())
}

/// Begin a request. Returns a draft id, or 0 if a draft could not be made.
#[no_mangle]
pub extern "C" fn js_perry_smtp_begin() -> i64 {
    let id = NEXT_DRAFT.with(|next| {
        let mut next = next.borrow_mut();
        let id = *next;
        *next = next.wrapping_add(1).max(1);
        id
    });
    DRAFTS.with(|drafts| drafts.borrow_mut().insert(id, Draft::default()));
    id
}

/// # Safety
/// `host_ptr` must be null or point at `host_len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn js_perry_smtp_set_server(
    draft: i64,
    host_ptr: *const u8,
    host_len: usize,
    port: u16,
    implicit_tls: i32,
    require_tls: i32,
) {
    // SAFETY: forwarded caller contract.
    let host = unsafe { text(host_ptr, host_len) };
    DRAFTS.with(|drafts| {
        if let Some(draft) = drafts.borrow_mut().get_mut(&draft) {
            draft.config.host = host;
            draft.config.port = port;
            draft.config.implicit_tls = implicit_tls != 0;
            draft.config.require_tls = require_tls != 0;
        }
    });
}

/// # Safety
/// Both pointers must be null or point at their stated lengths.
#[no_mangle]
pub unsafe extern "C" fn js_perry_smtp_set_auth(
    draft: i64,
    user_ptr: *const u8,
    user_len: usize,
    pass_ptr: *const u8,
    pass_len: usize,
) {
    // SAFETY: forwarded caller contract.
    let (user, pass) = unsafe { (text(user_ptr, user_len), text(pass_ptr, pass_len)) };
    DRAFTS.with(|drafts| {
        if let Some(draft) = drafts.borrow_mut().get_mut(&draft) {
            draft.config.user = (!user.is_empty()).then_some(user);
            draft.config.pass = (!pass.is_empty()).then_some(pass);
        }
    });
}

/// # Safety
/// `ptr` must be null or point at `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn js_perry_smtp_set_client_name(draft: i64, ptr: *const u8, len: usize) {
    // SAFETY: forwarded caller contract.
    let name = unsafe { text(ptr, len) };
    if name.is_empty() {
        return;
    }
    DRAFTS.with(|drafts| {
        if let Some(draft) = drafts.borrow_mut().get_mut(&draft) {
            draft.config.client_name = name;
        }
    });
}

/// # Safety
/// `ptr` must be null or point at `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn js_perry_smtp_set_from(draft: i64, ptr: *const u8, len: usize) {
    // SAFETY: forwarded caller contract.
    let from = unsafe { text(ptr, len) };
    DRAFTS.with(|drafts| {
        if let Some(draft) = drafts.borrow_mut().get_mut(&draft) {
            draft.from = from;
        }
    });
}

/// # Safety
/// `ptr` must be null or point at `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn js_perry_smtp_add_recipient(draft: i64, ptr: *const u8, len: usize) {
    // SAFETY: forwarded caller contract.
    let to = unsafe { text(ptr, len) };
    if to.is_empty() {
        return;
    }
    DRAFTS.with(|drafts| {
        if let Some(draft) = drafts.borrow_mut().get_mut(&draft) {
            draft.to.push(to);
        }
    });
}

/// # Safety
/// Both pointers must be null or point at their stated lengths.
#[no_mangle]
pub unsafe extern "C" fn js_perry_smtp_set_message(
    draft: i64,
    id_ptr: *const u8,
    id_len: usize,
    body_ptr: *const u8,
    body_len: usize,
) {
    // SAFETY: forwarded caller contract.
    let message_id = unsafe { text(id_ptr, id_len) };
    let message = if body_ptr.is_null() || body_len == 0 {
        Vec::new()
    } else {
        // SAFETY: forwarded caller contract; copied here.
        unsafe { std::slice::from_raw_parts(body_ptr, body_len) }.to_vec()
    };
    DRAFTS.with(|drafts| {
        if let Some(draft) = drafts.borrow_mut().get_mut(&draft) {
            draft.message_id = message_id;
            draft.message = message;
        }
    });
}

/// Submit the draft. `verify` non-zero runs `transporter.verify()` instead of a
/// delivery. Returns 1 when the engine accepted it (the callback WILL run
/// exactly once), 0 when it declined — in which case nothing was submitted and
/// the caller must keep its own transport.
#[no_mangle]
pub extern "C" fn js_perry_smtp_submit(
    draft: i64,
    verify: i32,
    ctx: usize,
    done: PerrySmtpDone,
) -> i32 {
    let Some(draft) = DRAFTS.with(|drafts| drafts.borrow_mut().remove(&draft)) else {
        return 0;
    };
    CALLBACKS.with(|map| map.borrow_mut().insert(ctx, done));
    let sink = Sink {
        ctx,
        on_done: dispatch_to_binding,
    };
    let accepted = if verify != 0 {
        super::verify(&draft.config, sink).is_ok()
    } else {
        let job = MailJob {
            from: draft.from,
            to: draft.to,
            message_id: draft.message_id,
            message: draft.message,
        };
        super::send(&draft.config, job, sink).is_ok()
    };
    if !accepted {
        CALLBACKS.with(|map| map.borrow_mut().remove(&ctx));
    }
    i32::from(accepted)
}

/// Abandon a draft that was built and then not submitted.
#[no_mangle]
pub extern "C" fn js_perry_smtp_cancel(draft: i64) {
    DRAFTS.with(|drafts| drafts.borrow_mut().remove(&draft));
}

/// The engine's sink for a binding-owned submission: repack the outcome as a
/// borrowed `PerrySmtpResult` and call the binding back.
fn dispatch_to_binding(ctx: usize, outcome: Outcome) {
    let Some(done) = CALLBACKS.with(|map| map.borrow_mut().remove(&ctx)) else {
        return;
    };
    // Every string the result borrows is kept alive by these locals for the
    // duration of the call, and nothing escapes it.
    let (ok, code, message, response, response_code, message_id, accepted, rejected) =
        match &outcome {
            Outcome::Sent(info) => (
                1,
                String::new(),
                info.response.clone(),
                info.response.clone(),
                i32::from(info.response_code),
                info.message_id.clone(),
                info.accepted.clone(),
                info.rejected
                    .iter()
                    .map(|r| r.recipient.clone())
                    .collect::<Vec<_>>(),
            ),
            Outcome::Verified => (
                1,
                String::new(),
                String::new(),
                String::new(),
                -1,
                String::new(),
                Vec::new(),
                Vec::new(),
            ),
            Outcome::Err(error) => (
                0,
                error.code.to_string(),
                error.message.clone(),
                error.response.clone(),
                error.response_code.map_or(-1, i32::from),
                String::new(),
                Vec::new(),
                Vec::new(),
            ),
        };
    let accepted_slices: Vec<PerrySmtpSlice> =
        accepted.iter().map(|s| PerrySmtpSlice::of(s)).collect();
    let rejected_slices: Vec<PerrySmtpSlice> =
        rejected.iter().map(|s| PerrySmtpSlice::of(s)).collect();
    let result = PerrySmtpResult {
        ok,
        response_code,
        code: PerrySmtpSlice::of(&code),
        message: PerrySmtpSlice::of(&message),
        response: PerrySmtpSlice::of(&response),
        message_id: PerrySmtpSlice::of(&message_id),
        accepted: accepted_slices.as_ptr(),
        accepted_len: accepted_slices.len(),
        rejected: rejected_slices.as_ptr(),
        rejected_len: rejected_slices.len(),
    };
    done(ctx, &result as *const PerrySmtpResult);
}
