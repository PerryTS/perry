#![allow(clippy::missing_safety_doc)]
//! Node-API host core (#8523).
//!
//! Addons see opaque `napi_value` tokens, never Perry heap addresses. Each
//! token carries an environment-local slot index and generation. Slots belong
//! to a strict handle-scope stack and are mutable GC roots, so a copying
//! collection rewrites the actual storage an addon will later read.
//!
//! The exported functions use Node-API's raw-pointer ABI. Their pointer
//! validity requirements are defined by `js_native_api.h`; each entry point
//! validates nullable arguments before dereferencing them.

mod functions;
mod scopes;
mod values;

use std::cell::RefCell;
use std::ffi::{c_char, c_void, CString};
use std::sync::atomic::{AtomicU64, Ordering};

pub use functions::*;
pub use scopes::*;
pub use values::*;

pub type NapiEnv = *mut c_void;
pub type NapiValue = *mut c_void;
pub type NapiHandleScope = *mut c_void;
pub type NapiEscapableHandleScope = *mut c_void;
pub type NapiRef = *mut c_void;
pub type NapiCallbackInfo = *mut c_void;

pub const NAPI_AUTO_LENGTH: usize = usize::MAX;
pub const NAPI_VERSION: u32 = 8;

#[repr(i32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NapiStatus {
    Ok = 0,
    InvalidArg = 1,
    ObjectExpected = 2,
    StringExpected = 3,
    NameExpected = 4,
    FunctionExpected = 5,
    NumberExpected = 6,
    BooleanExpected = 7,
    ArrayExpected = 8,
    GenericFailure = 9,
    PendingException = 10,
    Cancelled = 11,
    EscapeCalledTwice = 12,
    HandleScopeMismatch = 13,
    CallbackScopeMismatch = 14,
    QueueFull = 15,
    Closing = 16,
    BigintExpected = 17,
    DateExpected = 18,
    ArraybufferExpected = 19,
    DetachableArraybufferExpected = 20,
    WouldDeadlock = 21,
    NoExternalBuffersAllowed = 22,
    CannotRunJs = 23,
}

#[repr(C)]
pub struct NapiExtendedErrorInfo {
    pub error_message: *const c_char,
    pub engine_reserved: *mut c_void,
    pub engine_error_code: u32,
    pub error_code: NapiStatus,
}

#[derive(Clone, Copy)]
pub(crate) struct HandleSlot {
    pub value_bits: u64,
    pub generation: u32,
    pub scope_depth: u32,
    pub live: bool,
}

pub(crate) struct HandleToken {
    env_serial: u64,
    slot: u32,
    generation: u32,
}

pub(crate) struct ScopeToken {
    env_serial: u64,
    depth: u32,
    escapable: bool,
    escaped: bool,
    closed: bool,
}

pub(crate) struct ReferenceRecord {
    env_serial: u64,
    value_bits: u64,
    refcount: u32,
    deleted: bool,
}

pub(crate) struct NativeCallbackRecord {
    pub callback: usize,
    pub data: usize,
}

pub(crate) struct CallbackInfoRecord {
    pub env_serial: u64,
    pub args: Vec<NapiValue>,
    pub this_value: NapiValue,
    pub data: usize,
    pub new_target: NapiValue,
}

// The boxed records are intentional: their addresses are the opaque pointers
// returned to addon code and must survive growth of the owning vectors.
#[allow(clippy::vec_box)]
pub(crate) struct Env {
    serial: u64,
    owner: std::thread::ThreadId,
    slots: Vec<HandleSlot>,
    free_slots: Vec<u32>,
    // Tokens are intentional tombstones: their addon-visible addresses are
    // never reused, so an out-of-scope handle cannot alias a later value.
    tokens: Vec<Box<HandleToken>>,
    token_lookup: crate::fast_hash::PtrHashMap<usize, usize>,
    scopes: Vec<usize>,
    // Scope and reference records follow the same stable-address rule as
    // value tokens. Their compact backing slots/roots are released instead.
    scope_tokens: Vec<Box<ScopeToken>>,
    references: Vec<Box<ReferenceRecord>>,
    reference_lookup: crate::fast_hash::PtrHashMap<usize, usize>,
    callbacks: Vec<NativeCallbackRecord>,
    active_callback_infos: Vec<usize>,
    pending_exception_bits: Option<u64>,
    last_status: NapiStatus,
    last_error_message: CString,
    error_info: NapiExtendedErrorInfo,
}

impl Env {
    fn new(serial: u64) -> Self {
        let mut env = Self {
            serial,
            owner: std::thread::current().id(),
            slots: Vec::new(),
            free_slots: Vec::new(),
            tokens: Vec::new(),
            token_lookup: crate::fast_hash::new_ptr_hash_map(),
            scopes: Vec::new(),
            scope_tokens: Vec::new(),
            references: Vec::new(),
            reference_lookup: crate::fast_hash::new_ptr_hash_map(),
            callbacks: Vec::new(),
            active_callback_infos: Vec::new(),
            pending_exception_bits: None,
            last_status: NapiStatus::Ok,
            last_error_message: CString::new("napi_ok").unwrap(),
            error_info: NapiExtendedErrorInfo {
                error_message: std::ptr::null(),
                engine_reserved: std::ptr::null_mut(),
                engine_error_code: 0,
                error_code: NapiStatus::Ok,
            },
        };
        env.refresh_error_info();
        env
    }

    fn refresh_error_info(&mut self) {
        self.error_info.error_message = self.last_error_message.as_ptr();
        self.error_info.error_code = self.last_status;
    }

    fn set_status(&mut self, status: NapiStatus, message: &'static str) -> NapiStatus {
        self.last_status = status;
        self.last_error_message = CString::new(message).expect("static N-API error has no NUL");
        self.refresh_error_info();
        status
    }

    fn current_scope_depth(&self) -> u32 {
        self.scopes.len() as u32
    }

    fn add_handle_at_depth(&mut self, value_bits: u64, scope_depth: u32) -> NapiValue {
        let (slot, generation) = if let Some(slot) = self.free_slots.pop() {
            let record = &mut self.slots[slot as usize];
            debug_assert!(!record.live);
            record.value_bits = value_bits;
            record.scope_depth = scope_depth;
            record.live = true;
            (slot, record.generation)
        } else {
            let slot = self.slots.len() as u32;
            let generation = 1;
            self.slots.push(HandleSlot {
                value_bits,
                generation,
                scope_depth,
                live: true,
            });
            (slot, generation)
        };
        let mut token = Box::new(HandleToken {
            env_serial: self.serial,
            slot,
            generation,
        });
        let ptr = (&mut *token) as *mut HandleToken as NapiValue;
        self.token_lookup.insert(ptr as usize, self.tokens.len());
        self.tokens.push(token);
        ptr
    }

    fn add_handle(&mut self, value_bits: u64) -> NapiValue {
        self.add_handle_at_depth(value_bits, self.current_scope_depth())
    }

    fn token(&self, value: NapiValue) -> Option<&HandleToken> {
        if value.is_null() {
            return None;
        }
        let index = *self.token_lookup.get(&(value as usize))?;
        self.tokens.get(index).map(Box::as_ref)
    }

    fn value_bits(&self, value: NapiValue) -> Option<u64> {
        let token = self.token(value)?;
        if token.env_serial != self.serial {
            return None;
        }
        let slot = self.slots.get(token.slot as usize)?;
        (slot.live && slot.generation == token.generation).then_some(slot.value_bits)
    }

    fn invalidate_scope(&mut self, depth: u32) {
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if slot.live && slot.scope_depth >= depth {
                slot.live = false;
                slot.generation = slot.generation.wrapping_add(1).max(1);
                slot.value_bits = crate::value::TAG_UNDEFINED;
                self.free_slots.push(index as u32);
            }
        }
    }

    fn reference(&self, reference: NapiRef) -> Option<&ReferenceRecord> {
        if reference.is_null() {
            return None;
        }
        let index = *self.reference_lookup.get(&(reference as usize))?;
        self.references
            .get(index)
            .map(Box::as_ref)
            .filter(|record| record.env_serial == self.serial && !record.deleted)
    }

    fn reference_mut(&mut self, reference: NapiRef) -> Option<&mut ReferenceRecord> {
        if reference.is_null() {
            return None;
        }
        let index = *self.reference_lookup.get(&(reference as usize))?;
        self.references
            .get_mut(index)
            .map(Box::as_mut)
            .filter(|record| record.env_serial == self.serial && !record.deleted)
    }
}

static NEXT_ENV_SERIAL: AtomicU64 = AtomicU64::new(1);

crate::perry_thread_local! {
    static NODE_API_ENV: RefCell<Option<Box<Env>>> = const { RefCell::new(None) };
}

/// Return the current Perry agent's lazily-created Node-API environment.
pub fn current_env() -> NapiEnv {
    NODE_API_ENV.with(|cell| {
        let mut env = cell.borrow_mut();
        if env.is_none() {
            *env = Some(Box::new(Env::new(
                NEXT_ENV_SERIAL.fetch_add(1, Ordering::Relaxed),
            )));
        }
        env.as_deref_mut().unwrap() as *mut Env as NapiEnv
    })
}

pub(crate) fn with_env<R>(env: NapiEnv, f: impl FnOnce(&Env) -> R) -> Option<R> {
    if env.is_null() {
        return None;
    }
    NODE_API_ENV.with(|cell| {
        let borrowed = cell.borrow();
        let current = borrowed.as_deref()?;
        if !std::ptr::eq(current, env.cast::<Env>()) || current.owner != std::thread::current().id()
        {
            return None;
        }
        Some(f(current))
    })
}

/// `f` must not allocate in Perry's GC heap. Callers copy inputs out, drop the
/// borrow, allocate, then re-enter only to publish the resulting root slot.
pub(crate) fn with_env_mut<R>(env: NapiEnv, f: impl FnOnce(&mut Env) -> R) -> Option<R> {
    if env.is_null() {
        return None;
    }
    NODE_API_ENV.with(|cell| {
        let mut borrowed = cell.borrow_mut();
        let current = borrowed.as_deref_mut()?;
        if !std::ptr::eq(current, env.cast::<Env>()) || current.owner != std::thread::current().id()
        {
            return None;
        }
        Some(f(current))
    })
}

pub(crate) fn value_bits(env: NapiEnv, value: NapiValue) -> Result<u64, NapiStatus> {
    with_env(env, |env| env.value_bits(value))
        .flatten()
        .ok_or(NapiStatus::InvalidArg)
}

pub(crate) fn add_handle(env: NapiEnv, value_bits: u64) -> Result<NapiValue, NapiStatus> {
    with_env_mut(env, |env| env.add_handle(value_bits)).ok_or(NapiStatus::InvalidArg)
}

pub(crate) fn set_status(env: NapiEnv, status: NapiStatus, message: &'static str) -> NapiStatus {
    with_env_mut(env, |env| env.set_status(status, message)).unwrap_or(NapiStatus::InvalidArg)
}

pub(crate) fn ok(env: NapiEnv) -> NapiStatus {
    set_status(env, NapiStatus::Ok, "napi_ok")
}

pub(crate) fn pending_exception(env: NapiEnv) -> Option<u64> {
    with_env(env, |env| env.pending_exception_bits).flatten()
}

pub(crate) fn store_pending_exception(env: NapiEnv, bits: u64) -> NapiStatus {
    with_env_mut(env, |env| {
        env.pending_exception_bits = Some(bits);
        env.set_status(NapiStatus::PendingException, "an exception is pending")
    })
    .unwrap_or(NapiStatus::InvalidArg)
}

pub(crate) fn catch_value_call(env: NapiEnv, f: impl FnOnce() -> f64) -> Result<f64, NapiStatus> {
    match crate::exception::js_call_catching(f) {
        Ok(value) => Ok(value),
        Err(exception) => {
            store_pending_exception(env, exception.to_bits());
            Err(NapiStatus::PendingException)
        }
    }
}

/// Mark and rewrite every native-owned Node-API root.
pub fn scan_node_api_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    NODE_API_ENV.with(|cell| {
        let mut borrowed = cell.borrow_mut();
        let Some(env) = borrowed.as_deref_mut() else {
            return;
        };
        for slot in &mut env.slots {
            if slot.live {
                visitor.visit_nanbox_u64_slot(&mut slot.value_bits);
            }
        }
        if let Some(exception) = env.pending_exception_bits.as_mut() {
            visitor.visit_nanbox_u64_slot(exception);
        }
        for reference in &mut env.references {
            if !reference.deleted && reference.refcount > 0 {
                visitor.visit_nanbox_u64_slot(&mut reference.value_bits);
            }
        }
    });
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_last_error_info(
    env: NapiEnv,
    result: *mut *const NapiExtendedErrorInfo,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    match with_env_mut(env, |env| {
        env.refresh_error_info();
        &env.error_info as *const NapiExtendedErrorInfo
    }) {
        Some(info) => {
            *result = info;
            NapiStatus::Ok
        }
        None => NapiStatus::InvalidArg,
    }
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_version(env: NapiEnv, result: *mut u32) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    *result = NAPI_VERSION;
    ok(env)
}

#[cfg(test)]
pub(crate) fn reset_env_for_test() {
    NODE_API_ENV.with(|cell| *cell.borrow_mut() = None);
}

#[cfg(test)]
mod tests;
