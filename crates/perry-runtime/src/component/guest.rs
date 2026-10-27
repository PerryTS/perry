//! Runtime half of a Perry-authored library component. Its export table lives
//! in GC-owned private global state; the adapter table contains only image
//! bytes and code addresses. No JS value crosses the C boundary unrooted.
mod frame;
mod resources;
#[cfg(test)]
#[path = "guest/tests.rs"]
mod tests;

use super::*;
use crate::object::owned_state_bag::{self as bag, COMPONENT_EXPORTS, COMPONENT_GUEST};
use perry_abi::component_guest::{ComponentGuestOps, GUEST_ABI_VERSION};
use std::sync::atomic::{AtomicPtr, Ordering};

static GUEST_OPS: AtomicPtr<ComponentGuestOps> = AtomicPtr::new(std::ptr::null_mut());

/// # Safety
/// A non-null table and its contract bytes must be immutable and valid for the
/// image lifetime. Generated adapters supply static image data and code
/// addresses; neither the table nor its fields may reference Perry's GC heap.
#[no_mangle]
pub unsafe extern "C" fn js_component_set_guest_ops(ops: *const ComponentGuestOps) -> i32 {
    if ops.is_null()
        || (*ops).abi_version != GUEST_ABI_VERSION
        || (*ops).size != std::mem::size_of::<ComponentGuestOps>()
        || (*ops).contract.is_null()
    {
        return 0;
    }
    match GUEST_OPS.compare_exchange(
        std::ptr::null_mut(),
        ops.cast_mut(),
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => 1,
        Err(previous) => i32::from(previous == ops.cast_mut()),
    }
}

fn ops() -> &'static ComponentGuestOps {
    let ops = GUEST_OPS.load(Ordering::Acquire);
    if ops.is_null() {
        fail("This function requires a generated component guest adapter");
    }
    unsafe { &*ops }
}

fn maybe_registry() -> f64 {
    let global = crate::object::js_get_global_this();
    unsafe { bag::load(object_pointer(global).unwrap() as usize, COMPONENT_GUEST) }
}

fn registry() -> f64 {
    let registry = maybe_registry();
    if !JSValue::from_bits(registry.to_bits()).is_undefined() {
        return registry;
    }
    // Imported services can be used by module initialization before the
    // module registers its exports. The adapter's immutable world already
    // defines these imports; keep that contract in the same owned context.
    let ops = ops();
    let world = serde_json::from_slice(unsafe {
        std::slice::from_raw_parts(ops.contract, ops.contract_len)
    })
    .unwrap_or_else(|_| fail("Invalid canonical adapter manifest"));
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(context::publish(world, undefined()));
    let global = scope.root_nanbox_f64(crate::object::js_get_global_this());
    unsafe {
        bag::store(
            object_pointer(global.get_nanbox_f64()).unwrap() as usize,
            COMPONENT_GUEST,
            owner.get_nanbox_f64(),
        );
    }
    owner.get_nanbox_f64()
}

fn exports(registry: f64) -> f64 {
    unsafe {
        bag::load(
            object_pointer(registry).unwrap() as usize,
            COMPONENT_EXPORTS,
        )
    }
}

#[no_mangle]
pub extern "C" fn js_component_register(contract: f64, exports: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let contract_input = scope.root_nanbox_f64(contract);
    let exports_input = scope.root_nanbox_f64(exports);
    let mut contract = String::new();
    let mut expected = Value::Null;
    let mut world = Value::Null;
    let result = crate::exception::catch_js_throw(|| {
        let ops = ops();
        let owner = scope.root_nanbox_f64(registry());
        if !JSValue::from_bits(self::exports(owner.get_nanbox_f64()).to_bits()).is_undefined() {
            fail("Component exports are already registered");
        }
        contract = string(contract_input.get_nanbox_f64());
        world =
            serde_json::from_str(&contract).unwrap_or_else(|_| fail("Invalid WIT export manifest"));
        expected = serde_json::from_slice(unsafe {
            std::slice::from_raw_parts(ops.contract, ops.contract_len)
        })
        .unwrap_or_else(|_| fail("Invalid canonical adapter manifest"));
        if world != expected {
            fail("Perry exports do not match the canonical adapter's WIT world");
        }
        let exports = scope.root_nanbox_f64(context::snapshot_functions(
            &world,
            "exports",
            exports_input.get_nanbox_f64(),
        ));
        unsafe {
            bag::store(
                object_pointer(owner.get_nanbox_f64()).unwrap() as usize,
                COMPONENT_EXPORTS,
                exports.get_nanbox_f64(),
            );
        }
        undefined()
    });
    drop((contract, world, expected));
    finish(result)
}

#[no_mangle]
pub extern "C" fn js_component_import(interface: f64, function: f64, args: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let interface_input = scope.root_nanbox_f64(interface);
    let function_input = scope.root_nanbox_f64(function);
    let args = scope.root_nanbox_f64(args);
    let mut interface_name = String::new();
    let mut function_name = String::new();
    let mut frame = Box::new(frame::Frame::new());
    let result = crate::exception::catch_js_throw(|| {
        let ops = ops();
        let owner = scope.root_nanbox_f64(registry());
        interface_name = string(interface_input.get_nanbox_f64());
        function_name = string(function_input.get_nanbox_f64());
        let world = unsafe { &(*context::get(owner.get_nanbox_f64())).world };
        let interfaces = world["imports"].as_array().unwrap();
        let interface = interfaces
            .iter()
            .position(|i| i["name"] == interface_name)
            .unwrap_or_else(|| fail("Unknown component import interface"));
        let functions = interfaces[interface]["functions"].as_array().unwrap();
        let function = functions
            .iter()
            .position(|f| f["name"] == function_name)
            .unwrap_or_else(|| fail("Unknown component import function"));
        let count =
            array_pointer(args.get_nanbox_f64()).map(|args| crate::array::js_array_length(args));
        if functions[function]["params"].as_array().map(Vec::len)
            != count.map(|count| count as usize)
        {
            fail("Component import argument count mismatch");
        }
        let input = frame.add(args.get_nanbox_f64());
        let output = unsafe {
            (ops.import)(
                (&raw mut *frame).cast(),
                interface as u32,
                function as u32,
                input,
            )
        };
        if let Some(error) = frame.error() {
            crate::exception::js_throw(error);
        }
        frame.get(output)
    });
    frame.release_import();
    drop((interface_name, function_name));
    finish(result)
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_SET_OPS: unsafe extern "C" fn(*const ComponentGuestOps) -> i32 =
    js_component_set_guest_ops;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_REGISTER: extern "C" fn(f64, f64) -> f64 = js_component_register;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_IMPORT: extern "C" fn(f64, f64, f64) -> f64 = js_component_import;
