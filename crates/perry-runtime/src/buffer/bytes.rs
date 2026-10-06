//! Byte access over the current Buffer and TypedArray layouts.
//!
//! A borrow must end before allocation, JS, or a safepoint. Pins are owned by
//! the creating thread; native work can use the address, but completion must
//! release the pin on that thread. No placement policy lives in this module.
use crate::value::JSValue;
use std::marker::PhantomData;
use std::ptr::NonNull;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotBytes {
    Foreign,
    Detached,
    Frozen,
    PinLimit,
    UnstableForeign,
}

/// An allocation-free, callback-free scope. The invariant lifetime prevents
/// a Rust borrow from escaping `no_gc`. Mutable access additionally requires
/// exclusive access to the byte span (including all aliased JS views).
pub struct NoGc<'s>(PhantomData<&'s mut &'s ()>);
#[cfg(any(debug_assertions, test))]
thread_local! { static NO_GC_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) }; }

pub fn no_gc<R>(f: impl for<'s> FnOnce(&'s NoGc<'s>) -> R) -> R {
    #[cfg(any(debug_assertions, test))]
    struct Guard;
    #[cfg(any(debug_assertions, test))]
    impl Drop for Guard {
        fn drop(&mut self) {
            scope_leave();
        }
    }
    #[cfg(any(debug_assertions, test))]
    let _guard = {
        scope_enter();
        Guard
    };
    f(&NoGc(PhantomData))
}

pub(crate) fn scope_enter() {
    #[cfg(any(debug_assertions, test))]
    NO_GC_DEPTH.with(|n| n.set(n.get() + 1));
}
pub(crate) fn scope_leave() {
    #[cfg(any(debug_assertions, test))]
    NO_GC_DEPTH.with(|n| n.set(n.get() - 1));
}

#[cfg(test)]
pub(crate) fn sabotage(fault: &str) -> bool {
    std::env::var("PERRY_B1_SABOTAGE").ok().as_deref() == Some(fault)
}

#[cfg(test)]
thread_local! { static TEST_NATIVE_COPY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
#[cfg(test)]
pub(crate) struct NativeCopyTestGuard(bool);
#[cfg(test)]
impl NativeCopyTestGuard {
    pub(crate) fn new() -> Self {
        Self(TEST_NATIVE_COPY.with(|c| c.replace(true)))
    }
}
#[cfg(test)]
impl Drop for NativeCopyTestGuard {
    fn drop(&mut self) {
        TEST_NATIVE_COPY.with(|c| c.set(self.0));
    }
}

pub(crate) fn assert_allocation_allowed() {
    #[cfg(test)]
    if sabotage("no_gc_assert") {
        return;
    }
    #[cfg(any(debug_assertions, test))]
    NO_GC_DEPTH.with(|n| assert_eq!(n.get(), 0, "allocation inside bytes::no_gc"));
}

pub(crate) struct Span {
    pub ptr: *mut u8,
    pub len: usize,
    pub owner: usize,
}

pub(crate) fn span(value: f64, writable: bool) -> Result<Span, NotBytes> {
    let value = JSValue::from_bits(value.to_bits());
    if !value.is_pointer() {
        return Err(NotBytes::Foreign);
    }
    let addr = value.as_pointer::<u8>() as usize;
    let (ptr, len, owner) = if super::is_registered_buffer(addr) {
        let owner = super::view::backing_of(addr);
        if super::is_detached_buffer(owner) || super::view::is_out_of_bounds_view(addr) {
            return Err(NotBytes::Detached);
        }
        let b = addr as *const super::BufferHeader;
        (
            super::buffer_data(b) as *mut u8,
            unsafe { (*b).length as usize },
            owner,
        )
    } else if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
        let ta = addr as *const crate::typedarray::TypedArrayHeader;
        let owner = if let Some(meta) = crate::typedarray_view::view_meta_of(addr) {
            if super::is_detached_buffer(meta.backing)
                || crate::typedarray_view::is_view_out_of_bounds(addr)
            {
                return Err(NotBytes::Detached);
            }
            super::view::backing_of(meta.backing)
        } else if crate::native_arena::is_native_typed_view(ta) {
            let view = unsafe { &*crate::native_arena::native_view_from_typed_array(ta) };
            let owner = unsafe { &*view.owner };
            if owner.disposed != 0 || view.generation != owner.generation {
                return Err(NotBytes::Detached);
            }
            view.owner as usize
        } else {
            addr
        };
        let bytes = unsafe { crate::typedarray::typed_array_bytes(ta) }.ok_or(NotBytes::Foreign)?;
        (bytes.as_ptr() as *mut u8, bytes.len(), owner)
    } else {
        return Err(NotBytes::Foreign);
    };
    if writable {
        let h = unsafe { &*crate::gc::header_from_trusted_user_ptr(addr as *const u8) };
        if h._reserved & crate::gc::OBJ_FLAG_FROZEN != 0 {
            return Err(NotBytes::Frozen);
        }
    }
    Ok(Span {
        ptr: if len == 0 {
            NonNull::<u8>::dangling().as_ptr()
        } else {
            ptr
        },
        len,
        owner,
    })
}

pub fn bytes<'s>(value: f64, _: &'s NoGc<'s>) -> Result<&'s [u8], NotBytes> {
    let span = span(value, false)?;
    Ok(unsafe { std::slice::from_raw_parts(span.ptr, span.len) })
}

/// # Safety
/// No other reference or JS view may access these bytes during the borrow.
pub unsafe fn bytes_mut<'s>(value: f64, _: &'s NoGc<'s>) -> Result<&'s mut [u8], NotBytes> {
    let span = span(value, true)?;
    Ok(std::slice::from_raw_parts_mut(span.ptr, span.len))
}

// Bits 9..14 are unused by current byte cells and NativeArena owners. Bit 15
// preserves a pre-existing permanent GC pin. Nested byte pins share the owner;
// no address registry or latch is introduced. Overflow is refused, never wraps.
const PIN_ONE: u16 = 1 << 9;
const PIN_MASK: u16 = 0x7e00;
const WAS_PINNED: u16 = 0x8000;

pub(crate) fn has_pins(owner: usize) -> bool {
    unsafe {
        (*crate::gc::header_from_trusted_user_ptr(owner as *const u8))._reserved & PIN_MASK != 0
    }
}

/// A rooted owner and stable byte address. Release on the creating thread.
/// Engine-owned foreign spans without a retain protocol can only be borrowed.
/// The pointer may be used by native work while the owning thread runs JS;
/// native code is responsible for synchronising accesses to shared bytes.
pub struct Pinned {
    pub(crate) ptr: *mut u8,
    pub(crate) len: usize,
    owner: usize,
    thread: std::thread::ThreadId,
    _not_send: PhantomData<std::rc::Rc<()>>,
}
impl Pinned {
    pub fn as_ptr(&self) -> *const u8 {
        self.ptr
    }
    pub fn as_mut_ptr(&self) -> *mut u8 {
        self.ptr
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

pub fn pin(value: f64) -> Result<Pinned, NotBytes> {
    let span = span(value, true)?;
    // Rooting a wrapper cannot retain memory that an external engine can
    // reallocate. Refuse that guarantee until its owner has a retain protocol.
    if super::is_foreign_backed_buffer(span.owner) && !super::header::has_owned_backing(span.owner)
    {
        return Err(NotBytes::UnstableForeign);
    }

    unsafe {
        let h = crate::gc::header_from_trusted_user_ptr(span.owner as *const u8).cast_mut();
        if (*h)._reserved & PIN_MASK == PIN_MASK {
            return Err(NotBytes::PinLimit);
        }
        if !has_pins(span.owner) {
            if (*h).gc_flags & crate::gc::GC_FLAG_PINNED != 0 {
                (*h)._reserved |= WAS_PINNED;
            }
            // All CURRENT byte owners are old-arena, malloc, or shared. B6
            // must externalize young inline owners before enabling young births.
            #[cfg(test)]
            if !sabotage("owner_root") {
                crate::gc::pin_object_non_young(h);
            }
            #[cfg(not(test))]
            crate::gc::pin_object_non_young(h);
        }
        (*h)._reserved += PIN_ONE;
    }
    Ok(Pinned {
        ptr: span.ptr,
        len: span.len,
        owner: span.owner,
        thread: std::thread::current().id(),
        _not_send: PhantomData,
    })
}
impl Drop for Pinned {
    fn drop(&mut self) {
        assert_eq!(
            self.thread,
            std::thread::current().id(),
            "byte pin released on another thread"
        );
        unsafe {
            let h = crate::gc::header_from_trusted_user_ptr(self.owner as *const u8).cast_mut();
            assert!(has_pins(self.owner));
            (*h)._reserved -= PIN_ONE;
            if !has_pins(self.owner) {
                if (*h)._reserved & WAS_PINNED == 0 {
                    crate::gc::unpin_object(h);
                }
                (*h)._reserved &= !WAS_PINNED;
                if super::is_detached_buffer(self.owner) {
                    drop(super::header::take_owned_backing(self.owner));
                }
                if (*h).obj_type == crate::gc::GC_TYPE_NATIVE_ARENA_OWNER {
                    crate::native_arena::release_disposed_bytes(
                        self.owner as *mut crate::native_arena::NativeArenaOwnerHeader,
                    );
                }
            }
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug)]
pub enum Brand {
    Buffer = 0,
    Uint8Array = 1,
    ArrayBuffer = 2,
    DataView = 3,
}
#[derive(Clone, Copy)]
pub enum Init {
    Zero,
    Uninit,
}

fn allocate(brand: Brand, len: usize, init: Init) -> f64 {
    assert_allocation_allowed();
    let len = i32::try_from(len)
        .unwrap_or_else(|_| crate::typedarray::throw_range_error(b"Invalid buffer size"));
    #[cfg(test)]
    let native_fixture = TEST_NATIVE_COPY.with(|c| c.get());
    #[cfg(not(test))]
    let native_fixture = false;
    let ptr = if native_fixture {
        // Test the future B3 placement through EXACTLY the same consumer.
        super::buffer_alloc_owned(len as u32, len as u32)
    } else if matches!(brand, Brand::ArrayBuffer) {
        // ArrayBuffer already uses native storage on main. Keep that rule.
        super::js_array_buffer_new(len)
    } else {
        match init {
            Init::Zero => super::js_buffer_alloc(len, 0),
            Init::Uninit => super::js_buffer_alloc_unsafe(len),
        }
    };
    match brand {
        Brand::Buffer => (),
        Brand::Uint8Array => super::mark_as_uint8array(ptr as usize),
        Brand::ArrayBuffer => super::mark_as_array_buffer(ptr as usize),
        Brand::DataView => super::mark_as_data_view(ptr as usize),
    }
    f64::from_bits(JSValue::pointer(ptr.cast()).bits())
}

pub fn new_bytes(brand: Brand, len: usize, init: Init) -> (f64, Pinned) {
    let value = allocate(brand, len, init);
    (value, pin(value).expect("fresh bytes must be pinnable"))
}

pub fn from_slice(brand: Brand, input: &[u8]) -> f64 {
    let value = allocate(brand, input.len(), Init::Uninit);
    #[cfg(test)]
    if sabotage("inline_copy") {
        let cell = JSValue::from_bits(value.to_bits()).as_pointer::<u8>();
        // Poison the pointer word, then let the witness inspect it BEFORE
        // dereferencing it. This reproduces ump's corruption without a UAF.
        unsafe {
            std::ptr::copy_nonoverlapping(input.as_ptr(), cell.add(8), input.len());
        }
        return value;
    }
    no_gc(|scope| unsafe {
        bytes_mut(value, scope)
            .expect("fresh byte span")
            .copy_from_slice(input)
    });
    value
}
