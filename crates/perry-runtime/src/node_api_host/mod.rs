#![allow(clippy::missing_safety_doc)]
//! Node-API host core (#8523).
//!
//! Addons see opaque `napi_value` tokens, never Perry heap addresses. A token
//! is the pair (slot index, generation) packed into the pointer word itself:
//! the handle names its slot on the environment's handle stack, so resolving
//! it is an index and a compare, and making one allocates nothing. Slots are
//! pushed in order; a handle scope records the stack height at open and
//! closing it lowers the height back (V8's handle-block model). The live part
//! of the stack is a set of mutable GC roots, so a copying collection
//! rewrites the actual storage an addon will later read.
//!
//! The exported functions use Node-API's raw-pointer ABI. Their pointer
//! validity requirements are defined by `js_native_api.h`; each entry point
//! validates nullable arguments before dereferencing them.

mod async_work;
mod bigint;
mod buffers;
mod functions;
pub(crate) mod hooks;
mod lifecycle;
mod loader;
mod metadata;
mod modules;
mod promises;
mod properties;
mod scopes;
mod symbols;
mod tsfn;
mod values;

use std::cell::RefCell;
use std::ffi::{c_char, c_void, CString};
use std::sync::atomic::{AtomicU64, Ordering};

pub use async_work::*;
pub use bigint::*;
pub use buffers::*;
pub use functions::*;
pub use lifecycle::*;
pub use loader::*;
pub use metadata::*;
pub use modules::*;
pub use promises::*;
pub use properties::*;
pub use scopes::*;
pub use tsfn::*;
pub use values::*;

pub type NapiEnv = *mut c_void;
pub type NapiValue = *mut c_void;
pub type NapiHandleScope = *mut c_void;
pub type NapiEscapableHandleScope = *mut c_void;
pub type NapiRef = *mut c_void;
pub type NapiCallbackInfo = *mut c_void;
pub type NapiDeferred = *mut c_void;
pub type NapiAsyncWork = *mut c_void;
pub type NapiAsyncContext = *mut c_void;
pub type NapiCallbackScope = *mut c_void;
pub type NapiThreadsafeFunction = *mut c_void;
pub type NapiAsyncCleanupHookHandle = *mut c_void;

pub const NAPI_AUTO_LENGTH: usize = usize::MAX;
/// Highest Node-API version the host implements, and what `napi_get_version`
/// reports: Node 26's `NODE_API_SUPPORTED_VERSION_MAX` (#10456).
pub const NAPI_VERSION: u32 = 10;

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

/// One entry of the handle stack. `generation` is the write that filled it;
/// a `napi_value` reads the slot only while it names that same write.
#[derive(Clone, Copy)]
pub(crate) struct HandleSlot {
    pub value_bits: u64,
    pub generation: u32,
}

/// An open handle scope: the handle-stack height when it opened. An
/// escapable scope opens one slot higher; the slot below its base is the
/// parent's, reserved for the one handle it may escape.
pub(crate) struct HandleScopeRecord {
    base: usize,
    generation: u32,
    escapable: bool,
    escaped: bool,
}

/// The low half of an opaque handle word is the stack index plus one (so no
/// handle is null), the high half the generation of the write it names.
const HANDLE_INDEX_BITS: u32 = usize::BITS / 2;
const HANDLE_INDEX_MASK: usize = (1usize << HANDLE_INDEX_BITS) - 1;
const HANDLE_GENERATION_MASK: u32 = (usize::MAX >> HANDLE_INDEX_BITS) as u32;

fn encode_handle(index: usize, generation: u32) -> *mut c_void {
    debug_assert!(index < HANDLE_INDEX_MASK);
    (((generation as usize) << HANDLE_INDEX_BITS) | (index + 1)) as *mut c_void
}

fn decode_handle(handle: *mut c_void) -> Option<(usize, u32)> {
    let word = handle as usize;
    let index = (word & HANDLE_INDEX_MASK).checked_sub(1)?;
    Some((index, (word >> HANDLE_INDEX_BITS) as u32))
}

pub(crate) struct ReferenceRecord {
    env_serial: u64,
    value_bits: u64,
    /// `value_bits` is held weakly: the count is 0 and the value is an object
    /// or symbol. A weak target is not traced; a moving collection rewrites
    /// it and [`prune_dead_weak_references`] clears it to undefined when the
    /// target dies, so `napi_get_reference_value` answers NULL as in node.
    /// Primitive values cannot be weak targets and stay strong at count 0.
    weak: bool,
    refcount: u32,
    deleted: bool,
    /// A rooted WeakRef to the metadata payload and its scalar finalizer id;
    /// neither the object address nor the native payload is retained strongly.
    finalizer_link: Option<(u64, u64)>,
}

pub(crate) struct NativeCallbackRecord {
    pub callback: usize,
    pub data: usize,
    /// The addon that created the function; see [`modules::ModuleRecord`].
    pub module: Option<u32>,
}

pub(crate) struct CallbackInfoRecord {
    pub env_serial: u64,
    pub args: Vec<NapiValue>,
    pub this_value: NapiValue,
    pub data: usize,
    pub new_target: NapiValue,
}

/// A pending deferred. It is in `Env::deferreds` exactly while it is
/// unsettled: settling removes it, so the root scan walks pending promises
/// only.
pub(crate) struct DeferredRecord {
    env_serial: u64,
    promise_bits: u64,
}

// The boxed records are intentional: their addresses are the opaque pointers
// returned to addon code and must survive growth of the owning vectors.
#[allow(clippy::vec_box)]
pub(crate) struct Env {
    serial: u64,
    /// The handle stack. `slots[..handle_top]` are the live handles (GC
    /// roots); `slots[handle_top..]` are released handles that still read
    /// until a new handle overwrites them or a collection drops them.
    slots: Vec<HandleSlot>,
    handle_top: usize,
    /// The open handle scopes, innermost last.
    scopes: Vec<HandleScopeRecord>,
    /// The generation of the next handle or scope this environment makes.
    /// Every write takes a new one, so a released handle never reads a later
    /// value written to its slot. It starts at an environment-specific
    /// point, so a handle from an earlier environment does not alias either.
    next_generation: u32,
    // Reference records keep the stable-address rule: their addresses are the
    // opaque pointers addon code holds.
    references: Vec<Box<ReferenceRecord>>,
    reference_lookup: crate::fast_hash::PtrHashMap<usize, usize>,
    callbacks: Vec<NativeCallbackRecord>,
    active_callback_infos: Vec<usize>,
    deferreds: Vec<Box<DeferredRecord>>,
    deferred_lookup: crate::fast_hash::PtrHashMap<usize, usize>,
    async_contexts: Vec<Box<AsyncContextRecord>>,
    async_context_lookup: crate::fast_hash::PtrHashMap<usize, usize>,
    callback_scope_tokens: Vec<Box<CallbackScopeRecord>>,
    callback_scope_stack: Vec<usize>,
    cleanup_hooks: Vec<CleanupHookRecord>,
    async_cleanup_hooks: Vec<Box<AsyncCleanupHookRecord>>,
    async_cleanup_lookup: crate::fast_hash::PtrHashMap<usize, usize>,
    async_works: Vec<Box<AsyncWorkRecord>>,
    async_work_lookup: crate::fast_hash::PtrHashMap<usize, usize>,
    tsfns: Vec<std::sync::Arc<ThreadsafeFunctionInner>>,
    loaded_addons: Vec<LoadedAddon>,
    modules: Vec<ModuleRecord>,
    active_module: Option<u32>,
    currently_loading_filename: Option<String>,
    instance_data: Option<InstanceDataRecord>,
    shutting_down: bool,
    /// The next position in the environment cleanup queue, shared by cleanup
    /// hooks, async cleanup hooks and thread-safe functions.
    cleanup_order: u64,
    external_memory: i64,
    pending_exception_bits: Option<u64>,
    /// The last call's status and message: two stores per call. The
    /// NUL-terminated copy `napi_get_last_error_info` hands out is made there,
    /// its only reader, and only when the message changed.
    last_status: NapiStatus,
    last_message: &'static str,
    /// The message `last_error_message` holds a copy of.
    copied_message: &'static str,
    last_error_message: CString,
    error_info: NapiExtendedErrorInfo,
}

impl Env {
    fn new(serial: u64) -> Self {
        let mut env = Self {
            serial,
            slots: Vec::new(),
            handle_top: 0,
            scopes: Vec::new(),
            next_generation: ((serial as u32).wrapping_mul(0x9E37_79B9) & HANDLE_GENERATION_MASK)
                .max(1),
            references: Vec::new(),
            reference_lookup: crate::fast_hash::new_ptr_hash_map(),
            callbacks: Vec::new(),
            active_callback_infos: Vec::new(),
            deferreds: Vec::new(),
            deferred_lookup: crate::fast_hash::new_ptr_hash_map(),
            async_contexts: Vec::new(),
            async_context_lookup: crate::fast_hash::new_ptr_hash_map(),
            callback_scope_tokens: Vec::new(),
            callback_scope_stack: Vec::new(),
            cleanup_hooks: Vec::new(),
            async_cleanup_hooks: Vec::new(),
            async_cleanup_lookup: crate::fast_hash::new_ptr_hash_map(),
            async_works: Vec::new(),
            async_work_lookup: crate::fast_hash::new_ptr_hash_map(),
            tsfns: Vec::new(),
            loaded_addons: Vec::new(),
            modules: Vec::new(),
            active_module: None,
            currently_loading_filename: None,
            instance_data: None,
            shutting_down: false,
            cleanup_order: 0,
            external_memory: 0,
            pending_exception_bits: None,
            last_status: NapiStatus::Ok,
            last_message: "napi_ok",
            copied_message: "napi_ok",
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

    /// Publish the last status as `napi_extended_error_info`. A message is
    /// named by its address: the copy is redone only when the call recorded a
    /// different message (an equal text at another address copies once more,
    /// which is still correct).
    fn refresh_error_info(&mut self) {
        if !std::ptr::eq(self.copied_message, self.last_message) {
            self.copied_message = self.last_message;
            self.last_error_message =
                CString::new(self.last_message).expect("static N-API error has no NUL");
        }
        self.error_info.error_message = self.last_error_message.as_ptr();
        self.error_info.error_code = self.last_status;
    }

    #[inline]
    fn set_status(&mut self, status: NapiStatus, message: &'static str) -> NapiStatus {
        self.last_status = status;
        self.last_message = message;
        status
    }

    fn next_cleanup_order(&mut self) -> u64 {
        self.cleanup_order += 1;
        self.cleanup_order
    }

    fn take_generation(&mut self) -> u32 {
        let generation = self.next_generation;
        self.next_generation = (generation.wrapping_add(1) & HANDLE_GENERATION_MASK).max(1);
        generation
    }

    /// Write `value_bits` into stack slot `index` as a new handle. Any earlier
    /// handle naming that slot stops reading.
    fn write_slot(&mut self, index: usize, value_bits: u64) -> NapiValue {
        let generation = self.take_generation();
        let slot = HandleSlot {
            value_bits,
            generation,
        };
        if index < self.slots.len() {
            self.slots[index] = slot;
        } else {
            debug_assert_eq!(index, self.slots.len());
            self.slots.push(slot);
        }
        encode_handle(index, generation)
    }

    /// A new handle in the innermost open scope: the next stack slot.
    fn add_handle(&mut self, value_bits: u64) -> NapiValue {
        let index = self.handle_top;
        self.handle_top += 1;
        self.write_slot(index, value_bits)
    }

    /// A released slot still reads until a new handle reuses it or a
    /// collection runs (V8 moves only the handle-block top when a scope
    /// closes): a callback may close its own scope and still return the
    /// object it built there.
    fn value_bits(&self, value: NapiValue) -> Option<u64> {
        let (index, generation) = decode_handle(value)?;
        let slot = self.slots.get(index)?;
        (slot.generation == generation).then_some(slot.value_bits)
    }

    fn open_handle_scope(&mut self, escapable: bool) -> *mut c_void {
        if escapable {
            // The parent's slot for the escapee, below the new scope's base.
            self.add_handle(crate::value::TAG_UNDEFINED);
        }
        let generation = self.take_generation();
        self.scopes.push(HandleScopeRecord {
            base: self.handle_top,
            generation,
            escapable,
            escaped: false,
        });
        encode_handle(self.scopes.len() - 1, generation)
    }

    /// The innermost open scope, when `scope` names it.
    fn innermost_scope(&mut self, scope: *mut c_void) -> Option<&mut HandleScopeRecord> {
        let (index, generation) = decode_handle(scope)?;
        let innermost = self.scopes.len().checked_sub(1)?;
        let record = self.scopes.last_mut()?;
        (index == innermost && record.generation == generation).then_some(record)
    }

    /// Close the innermost scope: its handles are released, the stack height
    /// returns to the scope's base.
    fn close_innermost_scope(&mut self) {
        if let Some(record) = self.scopes.pop() {
            self.handle_top = record.base;
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

    fn deferred(&self, deferred: NapiDeferred) -> Option<&DeferredRecord> {
        if deferred.is_null() {
            return None;
        }
        let index = *self.deferred_lookup.get(&(deferred as usize))?;
        self.deferreds
            .get(index)
            .map(Box::as_ref)
            .filter(|record| record.env_serial == self.serial)
    }

    /// Settling ends a deferred (as in Node, where the deferred is freed by
    /// `napi_resolve_deferred`/`napi_reject_deferred`): its record leaves the
    /// list and the record that took its place is re-indexed. Returns the
    /// promise it held.
    fn take_deferred(&mut self, deferred: NapiDeferred) -> Option<u64> {
        let promise_bits = self.deferred(deferred)?.promise_bits;
        let index = self.deferred_lookup.remove(&(deferred as usize))?;
        self.deferreds.swap_remove(index);
        if let Some(moved) = self.deferreds.get(index) {
            let pointer = (&**moved) as *const DeferredRecord as usize;
            self.deferred_lookup.insert(pointer, index);
        }
        Some(promise_bits)
    }

    fn async_context(&self, context: NapiAsyncContext) -> Option<&AsyncContextRecord> {
        if context.is_null() {
            return None;
        }
        let index = *self.async_context_lookup.get(&(context as usize))?;
        self.async_contexts
            .get(index)
            .map(Box::as_ref)
            .filter(|record| record.env_serial == self.serial)
    }

    /// `napi_async_destroy` ends a context: its record leaves the list and
    /// the record that took its place is re-indexed. Returns its ids.
    fn take_async_context(
        &mut self,
        context: NapiAsyncContext,
    ) -> Option<crate::async_hook_slots::AsyncResourceIds> {
        let ids = self.async_context(context)?.ids;
        let index = self.async_context_lookup.remove(&(context as usize))?;
        self.async_contexts.swap_remove(index);
        if let Some(moved) = self.async_contexts.get(index) {
            let pointer = (&**moved) as *const AsyncContextRecord as usize;
            self.async_context_lookup.insert(pointer, index);
        }
        Some(ids)
    }
}

static NEXT_ENV_SERIAL: AtomicU64 = AtomicU64::new(1);

mod env_storage {
    use super::*;
    type Value = RefCell<Option<Box<Env>>>;
    type Storage = crate::tls_hot::HotCell<Value, { std::mem::needs_drop::<Value>() as usize }>;
    crate::__perry_thread_local_storage!(Storage, const { RefCell::new(None) });
    pub(super) fn resolve(idx: u32) -> Result<*mut u8, crate::tls_hot::AccessError> {
        STORAGE.try_with(|cell| {
            cell.arm_guard(idx);
            cell.value_addr()
        })
    }
    pub(super) static KEY: crate::tls_hot::HotKey<Value> = crate::tls_hot::HotKey::new(
        crate::tls_hot::SlotId::named(concat!(
            module_path!(),
            "::NODE_API_ENV@",
            line!(),
            ":",
            column!()
        )),
        crate::native_finalizer_hook::resolve_env_storage,
    );
}
use env_storage::KEY as NODE_API_ENV;

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

// The environment lives in this thread's TLS, so pointer identity with it
// proves both that `env` is live and that this is its thread.
pub(crate) fn with_env<R>(env: NapiEnv, f: impl FnOnce(&Env) -> R) -> Option<R> {
    if env.is_null() {
        return None;
    }
    NODE_API_ENV.with(|cell| {
        let borrowed = cell.borrow();
        let current = borrowed.as_deref()?;
        if !std::ptr::eq(current, env.cast::<Env>()) {
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
        if !std::ptr::eq(current, env.cast::<Env>()) {
            return None;
        }
        Some(f(current))
    })
}

/// One environment entry for a whole call: `f` does the call's work and
/// returns its status (via [`Env::set_status`]), recorded in the same borrow.
/// Like [`with_env_mut`], `f` must not allocate in Perry's GC heap.
#[inline]
pub(crate) fn enter(env: NapiEnv, f: impl FnOnce(&mut Env) -> NapiStatus) -> NapiStatus {
    with_env_mut(env, f).unwrap_or(NapiStatus::InvalidArg)
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
/// The name the host's root scanner is registered under.
pub(crate) const ROOT_SCANNER_NAME: &str = "crate::node_api_host::scan_node_api_roots_mut";

pub(crate) fn install() {
    hooks::install();
}

pub fn scan_node_api_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    NODE_API_ENV.with(|cell| {
        let mut borrowed = cell.borrow_mut();
        let Some(env) = borrowed.as_deref_mut() else {
            return;
        };
        // A released slot is not a root: the collection may free or move its
        // value, so its handle stops reading here. Only the live stack is
        // scanned.
        let top = env.handle_top;
        env.slots.truncate(top);
        for slot in &mut env.slots {
            visitor.visit_nanbox_u64_slot(&mut slot.value_bits);
        }
        if let Some(exception) = env.pending_exception_bits.as_mut() {
            visitor.visit_nanbox_u64_slot(exception);
        }
        // The list holds pending deferreds only (settling removes the record).
        for deferred in &mut env.deferreds {
            visitor.visit_nanbox_u64_slot(&mut deferred.promise_bits);
        }
        // Each native async context owns its token until napi_async_destroy,
        // including contexts created without a JavaScript resource value;
        // destroying it removes the record.
        for context in &mut env.async_contexts {
            crate::async_hook_slots::resource_state::scan_token(&mut context.ids.async_id, visitor);
        }
        for tsfn in &env.tsfns {
            tsfn::scan_tsfn_function_root(tsfn, visitor);
        }
        for addon in &mut env.loaded_addons {
            visitor.visit_nanbox_u64_slot(&mut addon.exports_bits);
        }
        for reference in &mut env.references {
            if reference.deleted {
                continue;
            }
            if reference.weak {
                let mut target = f64::from_bits(reference.value_bits);
                if visitor.visit_metadata_nanbox_f64_slot(&mut target) {
                    reference.value_bits = target.to_bits();
                }
            } else {
                visitor.visit_nanbox_u64_slot(&mut reference.value_bits);
            }
            if let Some((holder, _)) = reference.finalizer_link.as_mut() {
                visitor.visit_nanbox_u64_slot(holder);
            }
        }
    });
}

/// The heap address a weak reference's target occupies, if it has one.
fn weak_target_addr(bits: u64) -> Option<usize> {
    use crate::value::{BIGINT_TAG, POINTER_MASK, POINTER_TAG, STRING_TAG, TAG_MASK};
    if !matches!(bits & TAG_MASK, POINTER_TAG | STRING_TAG | BIGINT_TAG) {
        return None;
    }
    let addr = (bits & POINTER_MASK) as usize;
    (addr != 0 && !crate::value::addr_class::is_handle_band(addr)).then_some(addr)
}

/// Clear every weak reference whose target this collection found dead
/// (`gc::dead_owner::DEAD_KEY_PRUNES`). The target word becomes undefined,
/// which a weak record reads as "collected".
pub(crate) fn prune_dead_weak_references(is_dead: &dyn Fn(usize) -> bool) {
    NODE_API_ENV.with(|cell| {
        let mut borrowed = cell.borrow_mut();
        let Some(env) = borrowed.as_deref_mut() else {
            return;
        };
        for reference in &mut env.references {
            if reference.deleted || !reference.weak {
                continue;
            }
            if weak_target_addr(reference.value_bits).is_some_and(is_dead) {
                reference.value_bits = crate::value::TAG_UNDEFINED;
            }
        }
    });
}

/// Drain callbacks which must run on the owning Perry agent after GC or a
/// worker handoff. Called from the ordinary event pump.
pub fn process_pending() -> i32 {
    metadata::drain_pending_finalizers()
        .saturating_add(async_work::drain_async_completions())
        .saturating_add(tsfn::drain_threadsafe_functions())
}

pub fn has_active_work() -> bool {
    metadata::has_pending_finalizers()
        || async_work::has_active_async_work()
        || tsfn::has_active_threadsafe_functions()
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
#[cfg(test)]
mod v10_tests;

pub(crate) fn install_storage_resolvers() {
    crate::native_finalizer_hook::set_env_storage(env_storage::resolve);
    loader::install_legacy_storage();
    metadata::install_finalizer_storage();
    async_work::install_completion_queue();
}
