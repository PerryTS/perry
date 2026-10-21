use super::*;
use std::ffi::c_void;

const TYPE_ID: u64 = 0xe7e9_42a1_6ea2_04b6;
static VTABLE: crate::native_payload::PayloadVTable = crate::native_payload::PayloadVTable {
    drop: drop_context,
    stream: None,
};

pub(super) struct Context {
    pub host: *mut c_void,
    pub world: Value,
}

impl Drop for Context {
    fn drop(&mut self) {
        if !self.host.is_null() {
            unsafe { (host::ops().perry_component_host_drop)(self.host) }
        }
    }
}
unsafe extern "C" fn drop_context(pointer: *mut c_void, _: *mut c_void) {
    drop(Box::from_raw(pointer.cast::<Context>()));
}

pub(super) fn publish(world: Value, imports: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let imports = scope.root_nanbox_f64(imports);
    let context = Box::into_raw(Box::new(Context {
        host: std::ptr::null_mut(),
        world,
    }));
    let cell = unsafe {
        crate::native_handle::native_handle_new_rust_payload(
            context.cast(),
            TYPE_ID,
            &VTABLE,
            "Perry.Component",
            false,
        )
    };
    let cell = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(cell as i64));
    unsafe {
        crate::native_handle::native_handle_set_external_bytes(
            JSValue::from_bits(cell.get_nanbox_f64().to_bits())
                .as_pointer::<crate::native_handle::NativeHandleHeader>() as *mut _,
            std::mem::size_of::<Context>(),
        );
    }
    let object = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    let _publication = crate::gc::GcSuppressScope::new();
    unsafe {
        let pointer = object_pointer(object.get_nanbox_f64()).unwrap() as usize;
        crate::object::owned_state_bag::store(
            pointer,
            crate::object::owned_state_bag::COMPONENT_CONTEXT,
            cell.get_nanbox_f64(),
        );
        crate::object::owned_state_bag::store(
            pointer,
            crate::object::owned_state_bag::COMPONENT_IMPORTS,
            imports.get_nanbox_f64(),
        );
    }
    object.get_nanbox_f64()
}

pub(super) fn get(owner: f64) -> *mut Context {
    let pointer = object_pointer(owner).unwrap_or_else(|| fail("Expected a component handle"));
    let value = unsafe {
        crate::object::owned_state_bag::load(
            pointer as usize,
            crate::object::owned_state_bag::COMPONENT_CONTEXT,
        )
    };
    let value = JSValue::from_bits(value.to_bits());
    if !value.is_pointer() {
        fail("Expected a component handle");
    }
    let cell = value.as_pointer::<crate::native_handle::NativeHandleHeader>() as *mut _;
    let resource = unsafe { crate::native_handle::native_handle_rust_payload_ptr(cell, TYPE_ID) };
    if resource.is_null() {
        fail("Component handle is disposed or belongs to another thread");
    }
    resource.cast()
}

pub(super) fn imports(owner: f64) -> f64 {
    unsafe {
        crate::object::owned_state_bag::load(
            object_pointer(owner).unwrap() as usize,
            crate::object::owned_state_bag::COMPONENT_IMPORTS,
        )
    }
}

/// Only a completed disposal severs imports. A rejected reentrant disposal
/// leaves the store alive and it can still call these functions.
pub(super) fn finish_disposal(owner: f64, status: i32) {
    if matches!(
        status,
        perry_abi::component_host::OK | perry_abi::component_host::RELEASED_ERROR
    ) {
        unsafe {
            crate::object::owned_state_bag::store(
                object_pointer(owner).unwrap() as usize,
                crate::object::owned_state_bag::COMPONENT_IMPORTS,
                undefined(),
            );
        }
    }
}

pub(super) fn signature<'a>(
    world: &'a Value,
    side: &str,
    interface: &str,
    function: &str,
) -> &'a Value {
    world[side]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["name"] == interface))
        .and_then(|item| item["functions"].as_array())
        .and_then(|functions| functions.iter().find(|f| f["name"] == function))
        .unwrap_or_else(|| fail("Function is outside the selected WIT world"))
}

/// Resolve getters and check callable imports before entering Wasmtime. The
/// private snapshot roots the actual functions used for subsequent calls.
pub(super) fn snapshot_imports(world: &Value, input: f64, wasi: bool) -> f64 {
    snapshot_filtered(world, "imports", input, wasi)
}

pub(super) fn snapshot_functions(world: &Value, side: &str, input: f64) -> f64 {
    snapshot_filtered(world, side, input, false)
}

fn snapshot_filtered(world: &Value, side: &str, input: f64, wasi: bool) -> f64 {
    let scope = RuntimeHandleScope::new();
    let input = scope.root_nanbox_f64(input);
    let output = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    let interfaces = world[side]
        .as_array()
        .unwrap_or_else(|| fail("Invalid WIT import manifest"));
    for interface in interfaces {
        let name = interface["name"]
            .as_str()
            .unwrap_or_else(|| fail("Invalid WIT interface name"));
        if wasi && name.starts_with("wasi:") {
            continue;
        }
        let source = scope.root_nanbox_f64(if name.is_empty() {
            input.get_nanbox_f64()
        } else {
            field(input.get_nanbox_f64(), name.as_bytes())
        });
        let namespace = scope.root_nanbox_f64(if name.is_empty() {
            output.get_nanbox_f64()
        } else {
            crate::value::js_nanbox_pointer(crate::object::js_object_alloc(0, 0) as i64)
        });
        for function in interface["functions"]
            .as_array()
            .unwrap_or_else(|| fail("Invalid WIT function manifest"))
        {
            let function = function["name"]
                .as_str()
                .unwrap_or_else(|| fail("Invalid WIT function name"));
            let callback = field(source.get_nanbox_f64(), function.as_bytes());
            if !crate::object::instanceof::value_is_callable(callback) {
                fail(&format!(
                    "Missing or non-callable component import {name} / {function}"
                ));
            }
            set_field(namespace.get_nanbox_f64(), function.as_bytes(), callback);
        }
        if !name.is_empty() {
            set_field(
                output.get_nanbox_f64(),
                name.as_bytes(),
                namespace.get_nanbox_f64(),
            );
        }
    }
    output.get_nanbox_f64()
}
