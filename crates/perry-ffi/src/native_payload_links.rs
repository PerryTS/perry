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

/// Why a reopen did not install a payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachMiss {
    /// Foreign receiver or descriptor.
    Foreign,
    /// The cell still holds a payload.
    Open,
    /// A native call is releasing it.
    Closing,
    /// The cell was finalized.
    Finalized,
}

fn attach_code(rc: i32) -> Result<(), AttachMiss> {
    match rc {
        0 => Ok(()),
        5 => Err(AttachMiss::Open),
        6 => Err(AttachMiss::Closing),
        7 => Err(AttachMiss::Finalized),
        _ => Err(AttachMiss::Foreign),
    }
}

fn miss_code(code: i32) -> PayloadMiss {
    if code == -2 {
        PayloadMiss::Closed
    } else {
        PayloadMiss::Foreign
    }
}

extern "C" {
    fn js_perry_payload_link_ptr(link: usize, family: *const PayloadFamily, miss: *mut i32)
        -> *mut c_void;
    fn js_perry_payload_link_lifecycle(link: usize, family: *const PayloadFamily) -> i32;
    fn js_perry_payload_link_close(link: usize, family: *const PayloadFamily) -> i32;
    fn js_perry_payload_link_reopen(
        link: usize,
        family: *const PayloadFamily,
        resource: *mut c_void,
        bytes: usize,
    ) -> i32;
    fn js_perry_payload_link_js_state(link: usize, family: *const PayloadFamily, create: i32)
        -> f64;
    fn js_perry_payload_link_external_bytes(link: usize, family: *const PayloadFamily, bytes: usize);
    fn js_perry_payload_receiver_link(
        value: f64,
        family: *const PayloadFamily,
        miss: *mut i32,
    ) -> usize;
    fn js_perry_payload_proto_getter(
        proto: *mut c_void,
        name: *const u8,
        name_len: usize,
        info: *const crate::JsFunctionInfo,
    );
    fn js_perry_payload_proto_data(
        proto: *mut c_void,
        name: *const u8,
        name_len: usize,
        value: f64,
        flags: u32,
    );
    fn js_perry_payload_proto_inherit(proto: *mut c_void, parent: f64);
}

impl PayloadFamily {
    /// Set the constructor's `length`.
    pub const fn with_constructor_length(mut self, length: u32) -> Self {
        self.constructor_length = length;
        self
    }
}

/// Typed access with the miss kind: `Foreign` for another family, `Closed`
/// for a released or finalized cell.
/// # Safety
/// T is the descriptor's type. No allocation or JS while the reference lives.
pub unsafe fn payload_mut<'a, T>(
    value: f64,
    family: &'static PayloadFamily,
) -> Result<&'a mut T, PayloadMiss> {
    match get_mut::<T>(value, family) {
        Some(payload) => Ok(payload),
        None => match lifecycle(value, family) {
            Err(PayloadMiss::Foreign) => Err(PayloadMiss::Foreign),
            _ => Err(PayloadMiss::Closed),
        },
    }
}

/// Allocate on the family's prototype in `module` (empty for an internal
/// family with no export), then define `own` fields in order.
/// # Safety
/// T is the descriptor's payload type.
pub unsafe fn alloc_in<T>(
    family: &'static PayloadFamily,
    module: &str,
    payload: T,
    bytes: usize,
    own_fields: &[(&str, f64)],
) -> f64 {
    let proto = prototype(family, module);
    let value = alloc(family, payload, proto, bytes);
    for &(key, field) in own_fields {
        own(value, key, field);
    }
    value
}

/// The link of a receiver of this family (direct or subclass), OPEN or CLOSED.
pub fn receiver_link(value: f64, family: &'static PayloadFamily) -> Result<OwnerLink, PayloadMiss> {
    assert!(abi_matches(), "native payload ABI mismatch");
    let mut miss = 0;
    match unsafe { js_perry_payload_receiver_link(value, family, &mut miss) } {
        0 => Err(miss_code(miss)),
        raw => Ok(OwnerLink(raw)),
    }
}

/// The OPEN payload behind a link.
/// # Safety
/// A rooted owner or an outstanding ref keeps the cell alive. T is the
/// family's payload type. End the borrow before JS, release or reopen.
pub unsafe fn link_payload_ptr<T>(
    link: OwnerLink,
    family: &'static PayloadFamily,
) -> Result<*mut T, PayloadMiss> {
    let mut miss = 0;
    let ptr = js_perry_payload_link_ptr(link.0, family, &mut miss);
    if ptr.is_null() {
        Err(miss_code(miss))
    } else {
        Ok(ptr.cast())
    }
}

/// Lifecycle of the cell behind a link.
/// # Safety
/// As [`link_payload_ptr`].
pub unsafe fn link_lifecycle(
    link: OwnerLink,
    family: &'static PayloadFamily,
) -> Result<Lifecycle, PayloadMiss> {
    match js_perry_payload_link_lifecycle(link.0, family) {
        1 => Ok(Lifecycle::Open),
        2 => Ok(Lifecycle::Closing),
        3 => Ok(Lifecycle::Closed),
        code => Err(miss_code(code)),
    }
}

/// Release the payload behind a link; the cell and owner remain.
/// # Safety
/// As [`link_payload_ptr`].
pub unsafe fn close_link(link: OwnerLink, family: &'static PayloadFamily) -> bool {
    js_perry_payload_link_close(link.0, family) == 0
}

/// Reopen the CLOSED cell behind a link with a new payload.
/// # Safety
/// As [`link_payload_ptr`]; T is the family's payload type.
pub unsafe fn attach_link<T>(
    link: OwnerLink,
    family: &'static PayloadFamily,
    payload: T,
    bytes: usize,
) -> Result<(), AttachMiss> {
    let ptr = Box::into_raw(Box::new(payload));
    let result = attach_code(js_perry_payload_link_reopen(link.0, family, ptr.cast(), bytes));
    if result.is_err() {
        drop(Box::from_raw(ptr));
    }
    result
}

/// Attach a payload to a source subclass instance (its `super()` call).
/// # Safety
/// T is the family's payload type.
pub unsafe fn attach_to_object<T>(
    value: f64,
    family: &'static PayloadFamily,
    payload: T,
    bytes: usize,
) -> bool {
    attach(value, family, payload, bytes)
}

/// The owner's traced JS-state object, through its link.
/// # Safety
/// As [`link_payload_ptr`].
pub unsafe fn link_js_state(link: OwnerLink, family: &'static PayloadFamily, create: bool) -> f64 {
    js_perry_payload_link_js_state(link.0, family, create as i32)
}

/// Restate the native bytes the payload behind a link retains.
/// # Safety
/// As [`link_payload_ptr`].
pub unsafe fn link_set_external_bytes(link: OwnerLink, family: &'static PayloadFamily, bytes: usize) {
    js_perry_payload_link_external_bytes(link.0, family, bytes);
}

/// The prototype builder handed to a family's installer.
pub struct PayloadPrototype(*mut c_void);
impl PayloadPrototype {
    /// # Safety
    /// raw is the prototype handed to the installer.
    pub unsafe fn from_raw(raw: *mut c_void) -> Self {
        Self(raw)
    }
    /// A method.
    pub fn method(&mut self, name: &str, info: &'static crate::JsFunctionInfo, arity: u32) {
        unsafe { prototype_method(self.0, name, info, arity) }
    }
    /// An accessor (getter only).
    pub fn getter(&mut self, name: &str, info: &'static crate::JsFunctionInfo) {
        unsafe { js_perry_payload_proto_getter(self.0, name.as_ptr(), name.len(), info) }
    }
    /// A data property.
    pub fn data(&mut self, name: &str, value: f64, writable: bool, enumerable: bool, configurable: bool) {
        let flags = u32::from(writable) | u32::from(enumerable) << 1 | u32::from(configurable) << 2;
        unsafe { js_perry_payload_proto_data(self.0, name.as_ptr(), name.len(), value, flags) }
    }
    /// Link to a parent prototype.
    pub fn inherit(&mut self, parent: f64) {
        unsafe { js_perry_payload_proto_inherit(self.0, parent) }
    }
}
