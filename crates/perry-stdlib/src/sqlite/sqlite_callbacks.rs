//! The C trampolines SQLite calls for a `DatabaseSync`'s authorizer, scalar
//! functions and aggregates (class S), and for `applyChangeset`'s
//! `filter` / `onConflict` (class C).
//!
//! Rules (docs/native-payload-pattern.md, "Families whose native side calls
//! JS"): a trampoline never throws and never calls a throwing helper. It
//! finds its owner with `link_owner` (no JS once the database is closing,
//! closed or finalizing), reads the callback from the owner's `callbacks`
//! array after its last allocation, and calls JS only through
//! `call_from_link` / `call_from_native`, which park a throw on the owner.
//! Validation failures are parked with `set_pending_exception`. The entry
//! that made the C call rethrows after `finish`.

use super::*;
use perry_runtime::gc::{RuntimeHandle, RuntimeHandleScope};
use perry_runtime::native_payload::{self, CallbackSite, OwnerLink};
use perry_runtime::{
    buffer::bytes::{from_slice, Brand},
    js_array_get, js_array_length, js_string_from_bytes, ArrayHeader, JSValue,
};
use rusqlite::ffi;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};

perry_runtime::state_key_memo!(static MEMO_AGGSTATES);

/// The authorizer's index in the `callbacks` array. Functions and aggregates
/// take the following indices; an aggregate takes [`AGG_SLOTS`] of them.
pub(crate) const AUTHORIZER_INDEX: u32 = 0;
pub(crate) const AGG_START: u32 = 0;
pub(crate) const AGG_STEP: u32 = 1;
pub(crate) const AGG_RESULT: u32 = 2;
pub(crate) const AGG_INVERSE: u32 = 3;
pub(crate) const AGG_SLOTS: u32 = 4;
/// Set in a site's index when the registration asked for
/// `useBigIntArguments` (the site's plain data; the array index is below it).
pub(crate) const SITE_BIGINT_ARGS: u32 = 1 << 31;

/// A userdata site whose owner is open on this thread.
#[derive(Clone, Copy)]
struct Target {
    link: OwnerLink,
    index: u32,
    use_bigints: bool,
}

impl Target {
    /// The site's link and decoded index, unchecked.
    #[inline]
    fn of(site: &CallbackSite) -> Self {
        Target {
            link: site.link,
            index: site.index & !SITE_BIGINT_ARGS,
            use_bigints: site.index & SITE_BIGINT_ARGS != 0,
        }
    }

    /// The owner now (the GC may have moved it since the last look);
    /// `None` once a callback closed the database.
    unsafe fn owner(self) -> Option<f64> {
        native_payload::link_owner(self.link)
    }

    /// The callback at `index + offset`, read now.
    #[inline]
    unsafe fn callback(self, offset: u32) -> f64 {
        native_payload::callback_from_link(self.link, self.index + offset)
    }

    /// Park `error` on the owner (the first pending exception wins).
    unsafe fn park(self, error: f64) {
        if let Some(owner) = self.owner() {
            let _ = native_payload::set_pending_exception(owner, error);
        }
    }
}

/// The site a userdata pointer names; `None` once the database is closing,
/// closed or finalizing, or on a foreign thread.
unsafe fn site_target(user_data: *mut c_void) -> Option<Target> {
    if user_data.is_null() {
        return None;
    }
    let site = &*(user_data as *const CallbackSite);
    native_payload::link_owner(site.link)?;
    Some(Target::of(site))
}

unsafe fn array_element(array: f64, index: u32) -> f64 {
    if !value_from_f64(array).is_pointer() {
        return undefined_f64();
    }
    let arr = raw_addr_from_value(array) as *const ArrayHeader;
    if index >= js_array_length(arr) {
        return undefined_f64();
    }
    f64_from_jsvalue(js_array_get(arr, index))
}

unsafe fn result_error_pending(ctx: *mut ffi::sqlite3_context) {
    // The message never reaches JS: the parked exception is what the entry
    // throws.
    ffi::sqlite3_result_error(ctx, c"JavaScript callback threw".as_ptr(), -1);
}

fn range_error_value(message: &str) -> f64 {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    perry_runtime::node_submodules::register_error_code_pub(msg, "ERR_OUT_OF_RANGE");
    let err = perry_runtime::error::js_rangeerror_new(msg);
    perry_runtime::js_nanbox_pointer(err as i64)
}

fn plain_type_error_value(message: &str) -> f64 {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = perry_runtime::error::js_typeerror_new(msg);
    perry_runtime::js_nanbox_pointer(err as i64)
}

fn plain_range_error_value(message: &str) -> f64 {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = perry_runtime::error::js_rangeerror_new(msg);
    perry_runtime::js_nanbox_pointer(err as i64)
}

/// A SQLite integer as a JS value, or node's range error (not thrown).
pub(crate) unsafe fn integer_value_checked(value: i64, read_bigints: bool) -> Result<JSValue, f64> {
    if read_bigints {
        return Ok(JSValue::bigint_ptr(
            perry_runtime::bigint::js_bigint_from_i64(value),
        ));
    }
    if !(JS_SAFE_INTEGER_MIN..=JS_SAFE_INTEGER_MAX).contains(&value) {
        return Err(range_error_value(&format!(
            "Value is too large to be represented as a JavaScript number: {}",
            value
        )));
    }
    Ok(JSValue::number(value as f64))
}

unsafe fn arg_at(argv: *mut *mut ffi::sqlite3_value, index: usize) -> *mut ffi::sqlite3_value {
    if argv.is_null() {
        std::ptr::null_mut()
    } else {
        *argv.add(index)
    }
}

/// An argument whose conversion allocates nothing and cannot fail (null, a
/// float, a safe integer read as a number), or `None`.
unsafe fn immediate_arg(value: *mut ffi::sqlite3_value, use_bigints: bool) -> Option<f64> {
    if value.is_null() {
        return Some(f64_from_jsvalue(JSValue::null()));
    }
    match ffi::sqlite3_value_type(value) {
        ffi::SQLITE_NULL => Some(f64_from_jsvalue(JSValue::null())),
        ffi::SQLITE_FLOAT => Some(f64_from_jsvalue(JSValue::number(
            ffi::sqlite3_value_double(value),
        ))),
        ffi::SQLITE_INTEGER if !use_bigints => {
            let v = ffi::sqlite3_value_int64(value);
            (JS_SAFE_INTEGER_MIN..=JS_SAFE_INTEGER_MAX)
                .contains(&v)
                .then(|| f64_from_jsvalue(JSValue::number(v as f64)))
        }
        _ => None,
    }
}

/// One `sqlite3_value` argument as a JS value, without throwing.
unsafe fn value_arg_checked(
    value: *mut ffi::sqlite3_value,
    use_bigints: bool,
) -> Result<JSValue, f64> {
    if value.is_null() {
        return Ok(JSValue::null());
    }
    match ffi::sqlite3_value_type(value) {
        ffi::SQLITE_INTEGER => integer_value_checked(ffi::sqlite3_value_int64(value), use_bigints),
        ffi::SQLITE_FLOAT => Ok(JSValue::number(ffi::sqlite3_value_double(value))),
        ffi::SQLITE_TEXT => {
            let ptr = ffi::sqlite3_value_text(value);
            if ptr.is_null() {
                return Ok(JSValue::null());
            }
            let len = ffi::sqlite3_value_bytes(value) as usize;
            Ok(JSValue::string_ptr(js_string_from_bytes(ptr, len as u32)))
        }
        ffi::SQLITE_BLOB => {
            let len = ffi::sqlite3_value_bytes(value) as usize;
            let ptr = ffi::sqlite3_value_blob(value) as *const u8;
            let bytes: &[u8] = if len > 0 && !ptr.is_null() {
                std::slice::from_raw_parts(ptr, len)
            } else {
                &[]
            };
            Ok(JSValue::from_bits(
                from_slice(Brand::Uint8Array, bytes).to_bits(),
            ))
        }
        _ => Ok(JSValue::null()),
    }
}

/// Call `call` with the callback's arguments (`first` precedes them: an
/// aggregate's running state). When no argument allocates they are passed
/// from the stack with no handle scope; otherwise each is rooted as it is
/// built, and `call` runs after the last allocation (so it must read the
/// callee itself). `Err` carries a conversion error to park.
/// The arguments (after `first`) when none of them allocates: up to eight,
/// from the stack, with no handle scope.
#[inline]
unsafe fn immediate_args(
    first: Option<f64>,
    argc: c_int,
    argv: *mut *mut ffi::sqlite3_value,
    use_bigints: bool,
) -> Option<([f64; 8], usize)> {
    let argc = argc.max(0) as usize;
    let count = argc + usize::from(first.is_some());
    if count > 8 {
        return None;
    }
    let mut buf = [0f64; 8];
    let k = usize::from(first.is_some());
    if let Some(first) = first {
        buf[0] = first;
    }
    for i in 0..argc {
        buf[k + i] = immediate_arg(arg_at(argv, i), use_bigints)?;
    }
    Some((buf, count))
}

unsafe fn with_callback_args<R>(
    first: Option<f64>,
    argc: c_int,
    argv: *mut *mut ffi::sqlite3_value,
    use_bigints: bool,
    call: impl FnOnce(&[f64]) -> R,
) -> Result<R, f64> {
    if let Some((buf, count)) = immediate_args(first, argc, argv, use_bigints) {
        return Ok(call(&buf[..count]));
    }
    let argc = argc.max(0) as usize;
    let count = argc + usize::from(first.is_some());
    let scope = RuntimeHandleScope::new();
    let mut roots: Vec<RuntimeHandle<'_>> = Vec::with_capacity(count);
    if let Some(first) = first {
        roots.push(scope.root_nanbox_f64(first));
    }
    for i in 0..argc {
        let js = value_arg_checked(arg_at(argv, i), use_bigints)?;
        roots.push(scope.root_nanbox_u64(js.bits()));
    }
    let values: Vec<f64> = roots.iter().map(|root| root.get_nanbox_f64()).collect();
    Ok(call(&values))
}

pub(crate) unsafe extern "C" fn node_sqlite_scalar_trampoline(
    ctx: *mut ffi::sqlite3_context,
    argc: c_int,
    argv: *mut *mut ffi::sqlite3_value,
) {
    let user_data = ffi::sqlite3_user_data(ctx);
    if user_data.is_null() {
        ffi::sqlite3_result_error(ctx, c"database is not open".as_ptr(), -1);
        return;
    }
    let target = Target::of(&*(user_data as *const CallbackSite));
    // Arguments that allocate nothing go straight to the call, which checks
    // the owner. Any other conversion waits for an owner that may run JS.
    let result = match immediate_args(None, argc, argv, target.use_bigints) {
        Some((args, count)) => Ok(native_payload::call_callback(
            target.link,
            target.index,
            &args[..count],
        )),
        None => {
            if target.owner().is_none() {
                ffi::sqlite3_result_error(ctx, c"database is not open".as_ptr(), -1);
                return;
            }
            with_callback_args(None, argc, argv, target.use_bigints, |args| {
                native_payload::call_from_link(target.link, target.callback(0), args)
            })
        }
    };
    match result {
        Ok(Ok(value)) => node_sqlite_result_value(ctx, value),
        Ok(Err(())) => result_error_pending(ctx),
        Err(error) => {
            target.park(error);
            result_error_pending(ctx);
        }
    }
}

unsafe fn c_string_arg(scope: &RuntimeHandleScope, ptr: *const c_char) -> f64 {
    let value = sqlite_c_string_value(ptr);
    scope.root_nanbox_u64(value.bits()).get_nanbox_f64()
}

pub(crate) unsafe extern "C" fn node_sqlite_authorizer_trampoline(
    user_data: *mut c_void,
    action_code: c_int,
    arg1: *const c_char,
    arg2: *const c_char,
    db_name: *const c_char,
    trigger_or_view: *const c_char,
) -> c_int {
    let Some(target) = site_target(user_data) else {
        return ffi::SQLITE_DENY;
    };
    if !value_from_f64(target.callback(0)).is_pointer() {
        return ffi::SQLITE_OK;
    }
    let scope = RuntimeHandleScope::new();
    let args = [
        scope.root_nanbox_u64(JSValue::number(action_code as f64).bits()),
        scope.root_nanbox_f64(c_string_arg(&scope, arg1)),
        scope.root_nanbox_f64(c_string_arg(&scope, arg2)),
        scope.root_nanbox_f64(c_string_arg(&scope, db_name)),
        scope.root_nanbox_f64(c_string_arg(&scope, trigger_or_view)),
    ];
    let values = args.map(|arg| arg.get_nanbox_f64());
    let Ok(result) = native_payload::call_from_link(target.link, target.callback(0), &values)
    else {
        return ffi::SQLITE_DENY;
    };
    let result = value_from_f64(result);
    let code = if result.is_int32() {
        Some(result.as_int32())
    } else if result.is_number() {
        let number = result.as_number();
        (number.is_finite()
            && number.fract() == 0.0
            && number >= c_int::MIN as f64
            && number <= c_int::MAX as f64)
            .then_some(number as c_int)
    } else {
        None
    };
    let Some(code) = code else {
        target.park(plain_type_error_value(
            "Authorizer callback must return an integer authorization code",
        ));
        return ffi::SQLITE_DENY;
    };
    match code {
        ffi::SQLITE_OK | ffi::SQLITE_DENY | ffi::SQLITE_IGNORE => code,
        _ => {
            target.park(plain_range_error_value(
                "Authorizer callback returned a invalid authorization code",
            ));
            ffi::SQLITE_DENY
        }
    }
}

// ---- aggregates ------------------------------------------------------------
//
// The running accumulator of each aggregate group is a JS value, so it lives
// in the owner's `aggStates` JS-state array; SQLite's aggregate context holds
// only `slot + 1` (0 = the group has not started).

unsafe fn agg_states(owner: f64) -> f64 {
    native_payload::state_get_memo(owner, &DB_FAMILY, b"aggStates", &MEMO_AGGSTATES)
}

unsafe fn agg_get(target: Target, slot: u32) -> f64 {
    match target.owner() {
        Some(owner) => array_element(agg_states(owner), slot),
        None => undefined_f64(),
    }
}

unsafe fn agg_set(target: Target, slot: u32, value: f64) {
    let Some(owner) = target.owner() else {
        return;
    };
    let states = agg_states(owner);
    if value_from_f64(states).is_pointer() {
        let arr = raw_addr_from_value(states) as *mut ArrayHeader;
        if slot < js_array_length(arr) {
            // In range: a plain barriered store, nothing allocates.
            perry_runtime::array::js_array_set_f64(arr, slot, value);
            return;
        }
    }
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let owner = scope.root_nanbox_f64(owner);
    let mut states = agg_states(owner.get_nanbox_f64());
    if !value_from_f64(states).is_pointer() {
        let fresh = perry_runtime::js_nanbox_pointer(perry_runtime::js_array_alloc(0) as i64);
        native_payload::state_set_memo(
            owner.get_nanbox_f64(),
            &DB_FAMILY,
            b"aggStates",
            fresh,
            &MEMO_AGGSTATES,
        );
        states = agg_states(owner.get_nanbox_f64());
    }
    let arr = raw_addr_from_value(states) as *mut ArrayHeader;
    let updated = perry_runtime::array::js_array_set_f64_extend(arr, slot, value.get_nanbox_f64());
    if updated != arr {
        native_payload::state_set_memo(
            owner.get_nanbox_f64(),
            &DB_FAMILY,
            b"aggStates",
            perry_runtime::js_nanbox_pointer(updated as i64),
            &MEMO_AGGSTATES,
        );
    }
}

/// The group's state slot, starting the group (calling `start`) on first
/// use. `Err` when an exception is pending, the database closed, or SQLite is
/// out of memory.
unsafe fn agg_slot(ctx: *mut ffi::sqlite3_context, target: Target) -> Result<u32, ()> {
    let context =
        ffi::sqlite3_aggregate_context(ctx, std::mem::size_of::<u32>() as c_int) as *mut u32;
    if context.is_null() {
        ffi::sqlite3_result_error_nomem(ctx);
        return Err(());
    }
    if *context != 0 {
        return Ok(*context - 1);
    }
    let start = target.callback(AGG_START);
    let initial = if closure_ptr_from_value(start).is_some() {
        native_payload::call_from_link(target.link, start, &[])?
    } else {
        start
    };
    let scope = RuntimeHandleScope::new();
    let initial = scope.root_nanbox_f64(initial);
    let owner = target.owner().ok_or(())?;
    let slot = {
        let Ok(db) = native_payload::payload_mut::<NodeDb>(owner, &DB_FAMILY) else {
            return Err(());
        };
        match db.agg_free.pop() {
            Some(slot) => slot,
            None => {
                db.agg_len += 1;
                db.agg_len - 1
            }
        }
    };
    agg_set(target, slot, initial.get_nanbox_f64());
    *context = slot + 1;
    Ok(slot)
}

unsafe fn agg_release(ctx: *mut ffi::sqlite3_context, target: Target, slot: u32) {
    agg_set(target, slot, undefined_f64());
    if let Some(owner) = target.owner() {
        if let Ok(db) = native_payload::payload_mut::<NodeDb>(owner, &DB_FAMILY) {
            db.agg_free.push(slot);
        }
    }
    let context = ffi::sqlite3_aggregate_context(ctx, 0) as *mut u32;
    if !context.is_null() {
        *context = 0;
    }
}

unsafe fn aggregate_apply(
    ctx: *mut ffi::sqlite3_context,
    argc: c_int,
    argv: *mut *mut ffi::sqlite3_value,
    which: u32,
) {
    let Some(target) = site_target(ffi::sqlite3_user_data(ctx)) else {
        ffi::sqlite3_result_error(ctx, c"database is not open".as_ptr(), -1);
        return;
    };
    let Ok(slot) = agg_slot(ctx, target) else {
        result_error_pending(ctx);
        return;
    };
    let state = agg_get(target, slot);
    let result = with_callback_args(Some(state), argc, argv, target.use_bigints, |args| {
        native_payload::call_from_link(target.link, target.callback(which), args)
    });
    match result {
        Ok(Ok(next)) => agg_set(target, slot, next),
        Ok(Err(())) => result_error_pending(ctx),
        Err(error) => {
            target.park(error);
            result_error_pending(ctx);
        }
    }
}

pub(crate) unsafe extern "C" fn node_sqlite_aggregate_step(
    ctx: *mut ffi::sqlite3_context,
    argc: c_int,
    argv: *mut *mut ffi::sqlite3_value,
) {
    aggregate_apply(ctx, argc, argv, AGG_STEP);
}

pub(crate) unsafe extern "C" fn node_sqlite_aggregate_inverse(
    ctx: *mut ffi::sqlite3_context,
    argc: c_int,
    argv: *mut *mut ffi::sqlite3_value,
) {
    aggregate_apply(ctx, argc, argv, AGG_INVERSE);
}

unsafe fn aggregate_emit(ctx: *mut ffi::sqlite3_context, finalize: bool) {
    // xFinal also fires while the connection is being released (close, a
    // sweep or worker teardown): no JS then, and the payload drop discards
    // every state.
    let Some(target) = site_target(ffi::sqlite3_user_data(ctx)) else {
        ffi::sqlite3_result_null(ctx);
        return;
    };
    let Ok(slot) = agg_slot(ctx, target) else {
        result_error_pending(ctx);
        return;
    };
    let has_inverse = closure_ptr_from_value(target.callback(AGG_INVERSE)).is_some();
    let result_fn = target.callback(AGG_RESULT);
    let state = agg_get(target, slot);
    let value = if finalize && has_inverse {
        Ok(state)
    } else if closure_ptr_from_value(result_fn).is_some() {
        native_payload::call_from_link(target.link, result_fn, &[state])
    } else {
        Ok(state)
    };
    match value {
        Ok(value) => node_sqlite_result_value(ctx, value),
        Err(()) => result_error_pending(ctx),
    }
    if finalize {
        agg_release(ctx, target, slot);
    }
}

pub(crate) unsafe extern "C" fn node_sqlite_aggregate_final(ctx: *mut ffi::sqlite3_context) {
    aggregate_emit(ctx, true);
}

pub(crate) unsafe extern "C" fn node_sqlite_aggregate_value(ctx: *mut ffi::sqlite3_context) {
    aggregate_emit(ctx, false);
}

// ---- applyChangeset (class C) --------------------------------------------------

/// Per-call userdata of `applyChangeset`: the callbacks live for this call
/// only and are runtime handles of the entry's scope.
pub(crate) struct ChangesetApplyContext<'a, 's> {
    pub(crate) owner: &'a RuntimeHandle<'s>,
    pub(crate) filter: Option<&'a RuntimeHandle<'s>>,
    pub(crate) on_conflict: Option<&'a RuntimeHandle<'s>>,
}

pub(crate) unsafe extern "C" fn node_sqlite_changeset_filter(
    ctx: *mut c_void,
    table: *const c_char,
) -> c_int {
    let ctx = &*(ctx as *const ChangesetApplyContext<'_, '_>);
    let Some(filter) = ctx.filter else {
        return 1;
    };
    let scope = RuntimeHandleScope::new();
    let table = if table.is_null() {
        ""
    } else {
        CStr::from_ptr(table).to_str().unwrap_or("")
    };
    let table = scope.root_nanbox_u64(
        JSValue::string_ptr(js_string_from_bytes(table.as_ptr(), table.len() as u32)).bits(),
    );
    match native_payload::call_from_native(
        ctx.owner.get_nanbox_f64(),
        filter.get_nanbox_f64(),
        undefined_f64(),
        &[table.get_nanbox_f64()],
    ) {
        Ok(result) => (perry_runtime::value::js_is_truthy(result) != 0) as c_int,
        Err(()) => 0,
    }
}

pub(crate) unsafe extern "C" fn node_sqlite_changeset_conflict(
    ctx: *mut c_void,
    conflict: c_int,
    _iter: *mut ffi::sqlite3_changeset_iter,
) -> c_int {
    let ctx = &*(ctx as *const ChangesetApplyContext<'_, '_>);
    let Some(on_conflict) = ctx.on_conflict else {
        return ffi::SQLITE_CHANGESET_ABORT;
    };
    let Ok(result) = native_payload::call_from_native(
        ctx.owner.get_nanbox_f64(),
        on_conflict.get_nanbox_f64(),
        undefined_f64(),
        &[f64_from_jsvalue(JSValue::number(conflict as f64))],
    ) else {
        return ffi::SQLITE_CHANGESET_ABORT;
    };
    let result = value_from_f64(result);
    if result.is_int32() {
        return result.as_int32() as c_int;
    }
    if result.is_number() {
        let number = result.as_number();
        if number.is_finite()
            && number.fract() == 0.0
            && number >= c_int::MIN as f64
            && number <= c_int::MAX as f64
        {
            return number as c_int;
        }
    }
    ffi::SQLITE_CHANGESET_ABORT
}
