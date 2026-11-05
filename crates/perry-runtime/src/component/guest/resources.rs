//! Canonical handles are private capabilities. A frame validates all transfers
//! before consuming any owner; borrows retain their call or owner lifetime.
//! All heap references live in object-owned arrays or rooted frame slots.
use super::super::*;
use super::frame::Frame;
use crate::object::owned_state_bag::{self as bag, COMPONENT_GUEST_RESOURCE};
use std::ffi::c_void;

#[path = "resource_cleanup.rs"]
mod cleanup;
#[path = "resource_payload.rs"]
pub(super) mod payload;

fn array(value: f64) -> *mut crate::array::ArrayHeader {
    array_pointer(value).unwrap_or_else(|| fail("Invalid guest resource state"))
}

const OWNER: u32 = 0;
const TYPE: u32 = 1;
const HANDLE: u32 = 2;
const REP: u32 = 3;
const STATE: u32 = 4;
const LIFETIME: u32 = 5;
const PARENT: u32 = 6;
const BORROW_COUNT: u32 = 7;
const OWNED: f64 = 1.0;
const BORROWED: f64 = 0.0;
const TRANSFERRED: f64 = 2.0;
const DISPOSED: f64 = 3.0;

fn get(data: f64, index: u32) -> f64 {
    crate::array::js_array_get_f64(array(data), index)
}
fn set(data: f64, index: u32, value: f64) {
    crate::array::js_array_set_f64(array(data), index, value);
}
fn data(resource: f64) -> f64 {
    let pointer = object_pointer(resource).unwrap_or_else(|| fail("Expected a guest resource"));
    let data = unsafe { bag::load(pointer as usize, COMPONENT_GUEST_RESOURCE) };
    if array_pointer(data).is_none()
        || get(data, OWNER).to_bits() != super::maybe_registry().to_bits()
    {
        fail("Expected a resource from this component");
    }
    data
}
fn live(resource: f64) -> f64 {
    let data = data(resource);
    if get(data, STATE) >= TRANSFERRED {
        fail("Guest resource was disposed or transferred");
    }
    let lifetime = get(data, LIFETIME);
    if !JSValue::from_bits(lifetime.to_bits()).is_undefined() && get(lifetime, 0) != 1.0 {
        fail("Guest resource borrow has expired");
    }
    let parent = get(data, PARENT);
    if !JSValue::from_bits(parent.to_bits()).is_undefined()
        && get(self::data(parent), STATE) != OWNED
    {
        fail("Guest resource borrow owner is no longer live");
    }
    data
}
fn owner(resource: f64) -> f64 {
    let data = live(resource);
    let parent = get(data, PARENT);
    if JSValue::from_bits(parent.to_bits()).is_undefined() {
        resource
    } else {
        parent
    }
}

fn wrap(ty: u32, handle: u32, rep: u32, owned: bool, lifetime: f64, parent: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let lifetime = scope.root_nanbox_f64(lifetime);
    let parent = scope.root_nanbox_f64(parent);
    let registry = scope.root_nanbox_f64(super::registry());
    let fields = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc_with_length(8) as i64,
    ));
    for (index, value) in [
        (OWNER, registry.get_nanbox_f64()),
        (TYPE, ty as f64),
        (HANDLE, handle as f64),
        (REP, rep as f64),
        (STATE, if owned { OWNED } else { BORROWED }),
        (LIFETIME, lifetime.get_nanbox_f64()),
        (PARENT, parent.get_nanbox_f64()),
        (BORROW_COUNT, 0.0),
    ] {
        set(fields.get_nanbox_f64(), index, value);
    }
    let wrapper = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    unsafe {
        bag::store(
            object_pointer(wrapper.get_nanbox_f64()).unwrap() as usize,
            COMPONENT_GUEST_RESOURCE,
            fields.get_nanbox_f64(),
        );
    }
    if owned {
        for (name, info) in [
            ("borrow", crate::fn_info!(borrow, 0)),
            ("dispose", crate::fn_info!(dispose, 0)),
        ] {
            let function = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(info, 1));
            crate::closure::js_closure_set_capture_f64(
                function.get_raw_mut_ptr(),
                0,
                wrapper.get_nanbox_f64(),
            );
            crate::object::set_builtin_closure_non_constructable(
                function.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as usize,
            );
            set_field(
                wrapper.get_nanbox_f64(),
                name.as_bytes(),
                crate::value::js_nanbox_pointer(
                    function.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
                ),
            );
        }
    }
    if owned {
        cleanup::register(wrapper.get_nanbox_f64(), fields.get_nanbox_f64());
    }
    wrapper.get_nanbox_f64()
}

extern "C" fn borrow(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let resource = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    let fields = live(resource.get_nanbox_f64());
    if get(fields, STATE) != OWNED {
        fail("Only an owned guest resource can be borrowed");
    }
    wrap(
        get(fields, TYPE) as u32,
        get(fields, HANDLE) as u32,
        get(fields, REP) as u32,
        false,
        undefined(),
        resource.get_nanbox_f64(),
    )
}

extern "C" fn dispose(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let resource = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    let fields = data(resource.get_nanbox_f64());
    if get(fields, STATE) == DISPOSED {
        return undefined();
    }
    let fields = live(resource.get_nanbox_f64());
    if get(fields, STATE) != OWNED {
        fail("A borrowed guest resource cannot be disposed");
    }
    unborrowed(fields);
    cleanup::unregister(resource.get_nanbox_f64());
    release(data(resource.get_nanbox_f64()))
}

/// The private state survives its wrapper as FinalizationRegistry holdings.
/// Transfer changes this same state, so a late job cannot drop a recycled
/// canonical handle that now belongs to another wrapper.
fn release(fields: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let fields = scope.root_nanbox_f64(fields);
    let mut frame = Frame::allocate_guest_frame();
    let result = crate::exception::catch_js_throw(|| {
        if get(fields.get_nanbox_f64(), STATE) != OWNED {
            return undefined();
        }
        if get(fields.get_nanbox_f64(), OWNER).to_bits() != super::maybe_registry().to_bits() {
            return undefined();
        }
        unborrowed(fields.get_nanbox_f64());
        let ty = get(fields.get_nanbox_f64(), TYPE) as u32;
        let handle = get(fields.get_nanbox_f64(), HANDLE) as u32;
        // Canonical resource.drop consumes the handle even if its destructor
        // fails. Invalidate before entering it so a destructor cannot reenter.
        set(fields.get_nanbox_f64(), STATE, DISPOSED);
        unsafe {
            (super::ops().resource)((&mut frame as *mut Frame).cast(), ty, 1, handle);
        }
        if let Some(error) = frame.error() {
            crate::exception::js_throw(error);
        }
        undefined()
    });
    drop(frame);
    finish(result)
}

/// Stage an ownership transfer. Repeated arguments may alias even when they
/// are reached through different records, lists or borrowed wrapper objects.
fn lower(frame: &mut Frame, slot: u32, ty: u32, owned: bool, local: bool) -> u32 {
    let resource = frame.get(slot);
    let fields = live(resource);
    if get(fields, TYPE) != ty as f64 {
        fail("Guest resource type mismatch");
    }
    let identity = owner(resource).to_bits();
    let handle = if local && !owned {
        get(fields, REP) as u32
    } else {
        get(fields, HANDLE) as u32
    };
    if frame
        .transfers
        .iter()
        .any(|slot| owner(frame.get(*slot)).to_bits() == identity)
    {
        fail("Guest resource is transferred more than once or borrowed during transfer");
    }
    if owned {
        if get(fields, STATE) != OWNED {
            fail("Expected an owned guest resource");
        }
        unborrowed(fields);
        if frame
            .borrows
            .iter()
            .any(|slot| owner(frame.get(*slot)).to_bits() == identity)
        {
            fail("Guest resource is borrowed during transfer");
        }
        frame.transfers.push(slot);
    } else if !frame
        .borrows
        .iter()
        .chain(frame.committed_borrows.iter().map(|(slot, _)| slot))
        .any(|slot| owner(frame.get(*slot)).to_bits() == identity)
    {
        // Stage a second slot for native-only retirement. Frame::add roots the
        // state before growing its managed array; no JS bits above survive it.
        let fields = data(owner(frame.get(slot)));
        let state = frame.add(fields);
        frame.borrows.push(slot);
        frame.borrow_states.push(state);
    }
    handle
}

fn unborrowed(fields: f64) {
    if get(fields, BORROW_COUNT) != 0.0 {
        fail("Guest resource has outstanding canonical borrows");
    }
}

pub(super) fn commit(frame: &mut Frame) {
    // Revalidate all owners before mutation. A property getter used by later
    // argument lowering could have disposed an earlier staged owner.
    for slot in &frame.transfers {
        let fields = live(frame.get(*slot));
        if get(fields, STATE) != OWNED {
            fail("Guest resource transfer is no longer valid");
        }
        unborrowed(fields);
    }
    for slot in &frame.borrows {
        live(frame.get(*slot));
    }
    for state in &frame.borrow_states {
        if get(frame.get(*state), BORROW_COUNT) >= u32::MAX as f64 {
            fail("Too many canonical resource borrowers");
        }
    }
    frame.committed_borrows.reserve(frame.borrows.len());
    for slot in &frame.transfers {
        set(data(frame.get(*slot)), STATE, TRANSFERRED);
    }
    frame.transfers.clear();
    for (slot, state) in frame.borrows.iter().zip(&frame.borrow_states) {
        let fields = frame.get(*state);
        set(fields, BORROW_COUNT, get(fields, BORROW_COUNT) + 1.0);
        frame.committed_borrows.push((*slot, *state));
    }
    frame.borrows.clear();
    frame.borrow_states.clear();
}

/// Called only while the frame's traced owner is alive. Slots are private and
/// validated at commit; release needs neither the current registry nor JS.
pub(super) fn release_borrows(frame: &mut Frame) {
    for (_, state) in std::mem::take(&mut frame.committed_borrows) {
        let fields = frame.get(state);
        let count = get(fields, BORROW_COUNT);
        debug_assert!(count > 0.0);
        set(fields, BORROW_COUNT, count - 1.0);
    }
}

#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_resource_lift(
    pointer: *mut c_void,
    ty: u32,
    handle: u32,
    rep: u32,
    owned: i32,
) -> u32 {
    super::frame::frame(pointer).guard(|frame| {
        if owned == 0 {
            frame.retain_incoming_borrow(ty, handle);
        }
        let lifetime = if owned != 0 {
            undefined()
        } else {
            frame.lifetime()
        };
        let value = wrap(ty, handle, rep, owned != 0, lifetime, undefined());
        frame.add(value)
    })
}
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_resource_lower(
    pointer: *mut c_void,
    slot: u32,
    ty: u32,
    owned: i32,
    local: i32,
) -> u32 {
    super::frame::frame(pointer).guard(|frame| lower(frame, slot, ty, owned != 0, local != 0))
}
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_resource_commit(pointer: *mut c_void) {
    super::frame::frame(pointer).guard(commit);
}

#[cfg(test)]
#[path = "resource_tests.rs"]
mod tests;
