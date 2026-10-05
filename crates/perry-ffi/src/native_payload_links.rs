//! Async owner links for the shared payload ABI.
use super::*;

/// Stable inert cell address. A bare link is not a GC root.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OwnerLink(usize);
impl OwnerLink {
    /// Address packed into a transport token.
    pub fn raw(self) -> usize {
        self.0
    }
    /// Recover a link carried by an outstanding completion.
    /// # Safety
    /// raw is a live cell address kept by a matching link ref.
    pub unsafe fn from_raw(raw: usize) -> Self {
        Self(raw)
    }
}
/// State of the same cell across close and reopen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifecycle {
    /// Payload installed.
    Open,
    /// Release deferred until a native call returns.
    Closing,
    /// Payload released; the cell and owner remain.
    Closed,
}
/// A checked payload receiver could not be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PayloadMiss {
    /// Foreign receiver or descriptor.
    Foreign,
    /// Finalized or wrong-thread receiver.
    Closed,
}

extern "C" {
    fn js_perry_payload_reopen(
        value: f64,
        family: *const PayloadFamily,
        resource: *mut c_void,
        bytes: usize,
    ) -> i32;
    fn js_perry_payload_lifecycle(value: f64, family: *const PayloadFamily) -> i32;
    fn js_perry_payload_owner_link(value: f64, family: *const PayloadFamily) -> usize;
    fn js_perry_payload_link_ref(link: usize);
    fn js_perry_payload_link_unref(link: usize);
    fn js_perry_payload_link_event_owner(link: usize, out: *mut f64) -> i32;
    fn js_perry_payload_js_state(value: f64, family: *const PayloadFamily, create: i32) -> f64;
}
/// Allocate the family's typed cell in CLOSED state on its prototype.
/// # Safety
/// proto is the family's prototype.
pub unsafe fn alloc_closed(family: &'static PayloadFamily, proto: f64) -> f64 {
    assert!(abi_matches(), "native payload ABI mismatch");
    js_perry_payload_alloc(family, std::ptr::null_mut(), proto, 0)
}
/// Reopen the same typed cell; ownership transfers only on success.
/// # Safety
/// T is the family's payload type. No live native resource is overwritten.
pub unsafe fn reopen<T>(
    value: f64,
    family: &'static PayloadFamily,
    payload: T,
    bytes: usize,
) -> bool {
    assert!(abi_matches(), "native payload ABI mismatch");
    let ptr = Box::into_raw(Box::new(payload));
    if js_perry_payload_reopen(value, family, ptr.cast(), bytes) == 0 {
        true
    } else {
        drop(Box::from_raw(ptr));
        false
    }
}
/// Check a receiver's current lifecycle.
pub fn lifecycle(value: f64, family: &'static PayloadFamily) -> Result<Lifecycle, PayloadMiss> {
    assert!(abi_matches(), "native payload ABI mismatch");
    match unsafe { js_perry_payload_lifecycle(value, family) } {
        1 => Ok(Lifecycle::Open),
        2 => Ok(Lifecycle::Closing),
        3 => Ok(Lifecycle::Closed),
        -2 => Err(PayloadMiss::Closed),
        _ => Err(PayloadMiss::Foreign),
    }
}
/// Obtain a link on the creator thread, including after explicit close.
pub fn owner_link(value: f64, family: &'static PayloadFamily) -> Result<OwnerLink, PayloadMiss> {
    assert!(abi_matches(), "native payload ABI mismatch");
    let raw = unsafe { js_perry_payload_owner_link(value, family) };
    if raw == 0 {
        Err(PayloadMiss::Foreign)
    } else {
        Ok(OwnerLink(raw))
    }
}
/// Retain one outstanding completion.
/// # Safety
/// link is live on this thread.
pub unsafe fn link_ref(link: OwnerLink) {
    js_perry_payload_link_ref(link.0);
}
/// Release a matching outstanding completion ref.
/// # Safety
/// link is live on this thread and has a matching ref.
pub unsafe fn link_unref(link: OwnerLink) {
    js_perry_payload_link_unref(link.0);
}
/// Event owner, including after release. Root before any allocation or JS.
/// # Safety
/// link is live on its creator thread.
pub unsafe fn link_event_owner(link: OwnerLink) -> Option<f64> {
    let mut out = 0.0;
    (js_perry_payload_link_event_owner(link.0, &mut out) != 0).then_some(out)
}
/// Traced JS-state object, created on demand when requested.
pub fn js_state(value: f64, family: &'static PayloadFamily, create: bool) -> f64 {
    assert!(abi_matches(), "native payload ABI mismatch");
    unsafe { js_perry_payload_js_state(value, family, create as i32) }
}
