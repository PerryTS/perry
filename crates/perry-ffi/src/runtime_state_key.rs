//! Fixed-name extension accesses use the runtime's StateKeyMemo words.
use std::ffi::c_void;
use std::sync::atomic::{AtomicPtr, AtomicU64};

/// The C representation of the runtime's existing per-agent StateKeySite.
/// Declare one in thread_local! per fixed-key call site. Holder entries live
/// and are rooted in the runtime's existing PIC arena.
#[repr(C)]
pub struct StateKeySite {
    packed: AtomicU64,
    cache: AtomicPtr<c_void>,
}

extern "C-unwind" {
    fn js_runtime_state_key_get(
        value: f64,
        key: *const u8,
        len: usize,
        site: *const StateKeySite,
    ) -> f64;
}
extern "C-unwind" {
    fn js_runtime_state_key_call(
        value: f64,
        key: *const u8,
        len: usize,
        site: *const StateKeySite,
        args: *const f64,
        argc: usize,
    ) -> f64;
}

impl Default for StateKeySite {
    fn default() -> Self {
        Self::new()
    }
}
impl StateKeySite {
    /// An empty site with the emitted read's unmatched compact-word sentinel.
    pub const fn new() -> Self {
        Self {
            packed: AtomicU64::new(0xFFFF_FFFF),
            cache: AtomicPtr::new(std::ptr::null_mut()),
        }
    }
    /// Get this site's fixed name with ordinary JavaScript semantics.
    /// # Safety
    /// This site is used only for `name`, on its creating JS agent.
    pub unsafe fn get(&self, value: f64, name: &'static [u8]) -> f64 {
        js_runtime_state_key_get(value, name.as_ptr(), name.len(), self)
    }
    /// Call this site's fixed method with its receiver as `this`.
    /// # Safety
    /// As get; every argument is a live JavaScript value.
    pub unsafe fn call(&self, value: f64, name: &'static [u8], args: &[f64]) -> f64 {
        js_runtime_state_key_call(
            value,
            name.as_ptr(),
            name.len(),
            self,
            args.as_ptr(),
            args.len(),
        )
    }
}
