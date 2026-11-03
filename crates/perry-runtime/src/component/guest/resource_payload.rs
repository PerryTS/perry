//! Exported payloads stay rooted while a canonical host owns their resource.
//! The canonical representation is an opaque integer identity, not an address.
use super::*;
use crate::object::owned_state_bag::{COMPONENT_GUEST_NEXT_RESOURCE, COMPONENT_GUEST_RESOURCES};

fn map_pointer(value: f64) -> *mut crate::map::MapHeader {
    crate::map::map_ptr_from_receiver_bits(value.to_bits())
        .unwrap_or_else(|| fail("Invalid guest resource payload table"))
}
fn table(owner: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let existing = unsafe {
        bag::load(
            object_pointer(owner.get_nanbox_f64()).unwrap() as usize,
            COMPONENT_GUEST_RESOURCES,
        )
    };
    if !JSValue::from_bits(existing.to_bits()).is_undefined() {
        return existing;
    }
    let map = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::map::js_map_alloc(0) as i64
    ));
    unsafe {
        bag::store(
            object_pointer(owner.get_nanbox_f64()).unwrap() as usize,
            COMPONENT_GUEST_RESOURCES,
            map.get_nanbox_f64(),
        );
    }
    map.get_nanbox_f64()
}
fn matches_id(value: &Value, ty: u32) -> bool {
    value
        .as_str()
        .and_then(|s| s.strip_prefix('e'))
        .and_then(|s| s.parse::<u32>().ok())
        == Some(ty / 2)
}
fn local_type(owner: f64, ty: u32) -> bool {
    let world = unsafe { &(*context::get(owner)).world };
    let contains = |direction: &str| {
        world[direction]
            .as_array()
            .into_iter()
            .flatten()
            .any(|interface| {
                interface["resources"]
                    .as_object()
                    .into_iter()
                    .flat_map(|map| map.values())
                    .any(|key| matches_id(key, ty))
            })
    };
    ty % 2 == 1 && contains("exports")
}
fn type_id(value: f64) -> u32 {
    let value = JSValue::from_bits(value.to_bits());
    if !value.is_number() {
        fail("Invalid guest resource type");
    }
    let number = value.as_number();
    if number < 0.0 || number > u32::MAX as f64 || number.fract() != 0.0 {
        fail("Invalid guest resource type");
    }
    number as u32
}

#[no_mangle]
pub extern "C" fn js_component_resource_create(ty: f64, payload: f64, destructor: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let payload = scope.root_nanbox_f64(payload);
    let destructor = scope.root_nanbox_f64(destructor);
    let registry = scope.root_nanbox_f64(undefined());
    let map = scope.root_nanbox_f64(undefined());
    let record = scope.root_nanbox_f64(undefined());
    let mut rep = 0;
    let mut frame = Frame::allocate_guest_frame();
    let result = crate::exception::catch_js_throw(|| {
        let ty = type_id(ty);
        registry.set_nanbox_f64(super::super::registry());
        if !local_type(registry.get_nanbox_f64(), ty) {
            fail("Only guest-defined resources can be created");
        }
        if !JSValue::from_bits(destructor.get_nanbox_f64().to_bits()).is_undefined()
            && !crate::object::instanceof::value_is_callable(destructor.get_nanbox_f64())
        {
            fail("Guest resource destructor must be callable");
        }
        map.set_nanbox_f64(table(registry.get_nanbox_f64()));
        let next = unsafe {
            bag::load(
                object_pointer(registry.get_nanbox_f64()).unwrap() as usize,
                COMPONENT_GUEST_NEXT_RESOURCE,
            )
        };
        let next = if JSValue::from_bits(next.to_bits()).is_undefined() {
            1.0
        } else {
            next
        };
        if next > u32::MAX as f64 {
            fail("Guest resource identity space exhausted");
        }
        rep = next as u32;
        unsafe {
            bag::store(
                object_pointer(registry.get_nanbox_f64()).unwrap() as usize,
                COMPONENT_GUEST_NEXT_RESOURCE,
                next + 1.0,
            );
        }
        record.set_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::array::js_array_alloc_with_length(3) as i64,
        ));
        set(record.get_nanbox_f64(), 0, ty as f64);
        set(record.get_nanbox_f64(), 1, payload.get_nanbox_f64());
        set(record.get_nanbox_f64(), 2, destructor.get_nanbox_f64());
        crate::map::js_map_set(
            map_pointer(map.get_nanbox_f64()),
            rep as f64,
            record.get_nanbox_f64(),
        );
        let handle = unsafe {
            (super::super::ops().resource)((&mut frame as *mut Frame).cast(), ty, 0, rep)
        };
        if let Some(error) = frame.error() {
            crate::exception::js_throw(error);
        }
        wrap(ty, handle, rep, true, undefined(), undefined())
    });
    if result.is_err() && rep != 0 {
        crate::map::js_map_delete(map_pointer(map.get_nanbox_f64()), rep as f64);
    }
    drop(frame);
    finish(result)
}

#[no_mangle]
pub extern "C" fn js_component_resource_value(ty: f64, resource: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let resource = scope.root_nanbox_f64(resource);
    let ty = type_id(ty);
    let fields = live(resource.get_nanbox_f64());
    if get(fields, TYPE) != ty as f64 || get(fields, REP) == 0.0 {
        fail("Expected a guest-defined resource of this type");
    }
    let rep = get(fields, REP);
    let map = table(get(fields, OWNER));
    let record = crate::map::js_map_get(map_pointer(map), rep);
    if array_pointer(record).is_none() || get(record, 0) != ty as f64 {
        fail("Guest resource payload is no longer live");
    }
    get(record, 1)
}

#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_resource_destroy(
    pointer: *mut c_void,
    ty: u32,
    rep: u32,
) {
    super::super::frame::frame(pointer).guard::<()>(|_| {
        let scope = RuntimeHandleScope::new();
        let owner = scope.root_nanbox_f64(super::super::registry());
        let map = scope.root_nanbox_f64(table(owner.get_nanbox_f64()));
        let record = scope.root_nanbox_f64(crate::map::js_map_get(
            map_pointer(map.get_nanbox_f64()),
            rep as f64,
        ));
        if array_pointer(record.get_nanbox_f64()).is_none()
            || get(record.get_nanbox_f64(), 0) != ty as f64
        {
            fail("Unknown or already destroyed guest resource");
        }
        // Release the global root before user code, even if destruction throws.
        crate::map::js_map_delete(map_pointer(map.get_nanbox_f64()), rep as f64);
        let callback = scope.root_nanbox_f64(get(record.get_nanbox_f64(), 2));
        let payload = scope.root_nanbox_f64(get(record.get_nanbox_f64(), 1));
        if !JSValue::from_bits(callback.get_nanbox_f64().to_bits()).is_undefined() {
            let args = [payload.get_nanbox_f64()];
            crate::closure::js_native_call_value(
                callback.get_nanbox_f64(),
                crate::closure::plain_call_receiver(),
                args.as_ptr(),
                1,
            );
        }
    });
}
