//! Perry's typed component boundary. Native engines and guest canonical
//! adapters are optional; module code owns all JS roots and exceptions.
mod callback;
mod context;
mod guest;
mod host;
mod resource;
mod value;

#[cfg(test)]
mod tests;

use crate::{gc::RuntimeHandleScope, value::JSValue};
use perry_abi::component_host::Buffer;
use serde_json::Value;

fn undefined() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}
fn fail(message: &str) -> ! {
    crate::fs::validate::throw_error_with_code(message, "ERR_COMPONENT");
}
fn string(value: f64) -> String {
    crate::builtins::jsvalue_string_content(value)
        .unwrap_or_else(|| fail("Expected a string at the component boundary"))
}
fn object_pointer(value: f64) -> Option<*mut crate::object::ObjectHeader> {
    let value = JSValue::from_bits(value.to_bits());
    if !value.is_pointer() {
        return None;
    }
    let pointer = value.as_pointer::<u8>() as usize;
    let header = unsafe { crate::value::addr_class::try_read_tracked_gc_header(pointer)? };
    (unsafe { header.as_ref().obj_type } == crate::gc::GC_TYPE_OBJECT).then_some(pointer as *mut _)
}
fn array_pointer(value: f64) -> Option<*mut crate::array::ArrayHeader> {
    let value = JSValue::from_bits(value.to_bits());
    if !value.is_pointer() {
        return None;
    }
    let pointer = value.as_pointer::<u8>() as usize;
    let header = unsafe { crate::value::addr_class::try_read_tracked_gc_header(pointer)? };
    matches!(
        unsafe { header.as_ref().obj_type },
        crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_LAZY_ARRAY
    )
    .then_some(pointer as *mut _)
}
fn field(object: f64, name: &[u8]) -> f64 {
    let scope = RuntimeHandleScope::new();
    let object = scope.root_nanbox_f64(object);
    let key = scope.root_string_ptr(crate::string::js_string_from_bytes(
        name.as_ptr(),
        name.len() as u32,
    ));
    object_pointer(object.get_nanbox_f64())
        .map(|pointer| {
            crate::object::js_object_get_field_by_name_f64(pointer, key.get_raw_const_ptr())
        })
        .unwrap_or_else(undefined)
}
fn set_field(object: f64, name: &[u8], value: f64) {
    let scope = RuntimeHandleScope::new();
    let object = scope.root_nanbox_f64(object);
    let value = scope.root_nanbox_f64(value);
    let key = scope.root_string_ptr(crate::string::js_string_from_bytes(
        name.as_ptr(),
        name.len() as u32,
    ));
    crate::object::js_object_set_field_by_name(
        object_pointer(object.get_nanbox_f64()).unwrap(),
        key.get_raw_const_ptr(),
        value.get_nanbox_f64(),
    );
}
unsafe fn take(buffer: Buffer) -> Vec<u8> {
    let bytes = if buffer.data.is_null() {
        Vec::new()
    } else {
        std::slice::from_raw_parts(buffer.data, buffer.len).to_vec()
    };
    (host::ops().perry_component_host_buffer_drop)(buffer);
    bytes
}

// JS throws abandon Rust frames. Keep all owned native scratch outside the
// catching closure and destroy it before propagating the original exception.
fn finish(result: Result<f64, f64>) -> f64 {
    result.unwrap_or_else(|error| crate::exception::js_throw(error))
}

#[no_mangle]
pub extern "C" fn js_component_load(bytes: f64, contract: f64, imports: f64, options: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let bytes = scope.root_nanbox_f64(bytes);
    let contract_input = scope.root_nanbox_f64(contract);
    let imports = scope.root_nanbox_f64(imports);
    let options_input = scope.root_nanbox_f64(options);
    let owner = scope.root_nanbox_f64(undefined());
    let mut contract = String::new();
    let mut world = Value::Null;
    let mut wasm = Vec::new();
    let mut options = Vec::new();
    let mut error = None;
    let result = crate::exception::catch_js_throw(|| {
        let ops = host::ops();
        contract = string(contract_input.get_nanbox_f64());
        world =
            serde_json::from_str(&contract).unwrap_or_else(|_| fail("Invalid WIT manifest JSON"));
        let imports =
            scope.root_nanbox_f64(context::snapshot_imports(&world, imports.get_nanbox_f64()));
        owner.set_nanbox_f64(context::publish(
            std::mem::take(&mut world),
            imports.get_nanbox_f64(),
        ));
        let frame = callback::Frame::new(&scope, owner.get_nanbox_f64());
        // Copy guest bytes before callbacks can collect Perry's heap.
        wasm = crate::buffer::bytes::no_gc(|no_gc| {
            crate::buffer::bytes::bytes(bytes.get_nanbox_f64(), no_gc).map(<[u8]>::to_vec)
        })
        .unwrap_or_else(|_| fail("Expected component bytes"));
        options = if JSValue::from_bits(options_input.get_nanbox_f64().to_bits()).is_undefined() {
            b"{}".to_vec()
        } else {
            serde_json::to_vec(&value::encode(options_input.get_nanbox_f64())).unwrap()
        };
        let mut buffer = Buffer::default();
        unsafe {
            let pointer = (ops.perry_component_host_new)(
                wasm.as_ptr(),
                wasm.len(),
                contract.as_ptr(),
                contract.len(),
                options.as_ptr(),
                options.len(),
                Some(callback::invoke),
                Some(callback::free),
                frame.token(),
                &mut buffer,
            );
            (*context::get(owner.get_nanbox_f64())).host = pointer;
            if pointer.is_null() {
                error = Some(take(buffer));
            }
        }
        frame.rethrow();
        if let Some(error) = &error {
            fail(&String::from_utf8_lossy(error));
        }
        owner.get_nanbox_f64()
    });
    drop((contract, world, wasm, options, error));
    finish(result)
}

#[no_mangle]
pub extern "C" fn js_component_call(owner: f64, interface: f64, function: f64, args: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let interface_input = scope.root_nanbox_f64(interface);
    let function_input = scope.root_nanbox_f64(function);
    let args_input = scope.root_nanbox_f64(args);
    let frame = callback::Frame::new(&scope, owner.get_nanbox_f64());
    let mut interface = String::new();
    let mut function = String::new();
    let mut args = Vec::new();
    let mut output = Vec::new();
    let mut parsed = Value::Null;
    let result = crate::exception::catch_js_throw(|| {
        interface = string(interface_input.get_nanbox_f64());
        function = string(function_input.get_nanbox_f64());
        let context = context::get(owner.get_nanbox_f64());
        if unsafe { (*context).host.is_null() } {
            fail("Component is not initialized");
        }
        args = serde_json::to_vec(&value::encode(args_input.get_nanbox_f64())).unwrap();
        let mut buffer = Buffer::default();
        let status = unsafe {
            (host::ops().perry_component_host_call)(
                (*context).host,
                interface.as_ptr(),
                interface.len(),
                function.as_ptr(),
                function.len(),
                args.as_ptr(),
                args.len(),
                frame.token(),
                &mut buffer,
            )
        };
        output = unsafe { take(buffer) };
        frame.rethrow();
        if status != perry_abi::component_host::OK {
            fail(&String::from_utf8_lossy(&output));
        }
        parsed =
            serde_json::from_slice(&output).unwrap_or_else(|_| fail("Invalid component result"));
        if parsed["returned"] == true {
            let world = unsafe { &(*context).world };
            let signature = context::signature(world, "exports", &interface, &function);
            value::decode(
                &parsed["value"],
                &signature["result"],
                world,
                owner.get_nanbox_f64(),
            )
        } else {
            undefined()
        }
    });
    drop((interface, function, args, output, parsed));
    finish(result)
}

#[no_mangle]
pub extern "C" fn js_component_dispose(owner: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let frame = callback::Frame::new(&scope, owner);
    let mut error = Vec::new();
    let result = crate::exception::catch_js_throw(|| {
        let context = context::get(frame.owner.get_nanbox_f64());
        let mut buffer = Buffer::default();
        let status = unsafe {
            (host::ops().perry_component_host_dispose)((*context).host, frame.token(), &mut buffer)
        };
        error = unsafe { take(buffer) };
        // A destructor may have thrown in a JS import. Its exception is
        // rooted by frame, so sever imports before rethrowing that value.
        context::finish_disposal(frame.owner.get_nanbox_f64(), status);
        frame.rethrow();
        if status != perry_abi::component_host::OK {
            fail(&String::from_utf8_lossy(&error));
        }
        undefined()
    });
    drop(error);
    finish(result)
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_LOAD: extern "C" fn(f64, f64, f64, f64) -> f64 = js_component_load;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_CALL: extern "C" fn(f64, f64, f64, f64) -> f64 = js_component_call;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_DISPOSE: extern "C" fn(f64) -> f64 = js_component_dispose;
