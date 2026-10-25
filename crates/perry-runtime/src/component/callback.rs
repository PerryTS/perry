use super::*;
use crate::gc::RuntimeHandle;
use std::cell::Cell;

mod batch;
pub(super) mod saved;
#[cfg(test)]
mod saved_tests;
pub(super) use batch::FrameBatch;

pub(super) enum FrameValue<'a> {
    Root(RuntimeHandle<'a>),
    Row(batch::RowSlot<'a>),
}
impl FrameValue<'_> {
    pub fn get_nanbox_f64(&self) -> f64 {
        match self {
            Self::Root(root) => root.get_nanbox_f64(),
            Self::Row(slot) => slot.get_nanbox_f64(),
        }
    }
    pub fn set_nanbox_f64(&self, value: f64) {
        match self {
            Self::Root(root) => root.set_nanbox_f64(value),
            Self::Row(slot) => slot.set_nanbox_f64(value),
        }
    }
}

pub(super) struct Frame<'a> {
    pub owner: FrameValue<'a>,
    pub error: FrameValue<'a>,
    pub threw: Cell<bool>,
}

impl<'a> Frame<'a> {
    pub fn new(scope: &'a RuntimeHandleScope, owner: f64) -> Self {
        Self {
            owner: FrameValue::Root(scope.root_nanbox_f64(owner)),
            error: FrameValue::Root(scope.root_nanbox_f64(undefined())),
            threw: Cell::new(false),
        }
    }
    pub fn token(&self) -> u64 {
        self as *const Self as usize as u64
    }
    pub fn capture(&self, error: f64) {
        if !self.threw.get() {
            self.error.set_nanbox_f64(error);
            self.threw.set(true);
        }
    }
    pub fn rethrow(&self) {
        if self.threw.get() {
            crate::exception::js_throw(self.error.get_nanbox_f64());
        }
    }
}

fn reply(output: *mut Buffer, value: Vec<u8>) {
    let value = value.into_boxed_slice();
    unsafe {
        *output = Buffer {
            len: value.len(),
            data: Box::into_raw(value).cast(),
        };
    }
}

pub(super) unsafe extern "C" fn free(buffer: Buffer) {
    if !buffer.data.is_null() {
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
            buffer.data,
            buffer.len,
        )));
    }
}

pub(super) unsafe extern "C" fn invoke(
    token: u64,
    interface: *const u8,
    interface_len: usize,
    function: *const u8,
    function_len: usize,
    args: *const u8,
    args_len: usize,
    output: *mut Buffer,
) -> i32 {
    let frame = &*(token as usize as *const Frame<'_>);
    let scope = RuntimeHandleScope::new();
    let lifetime = scope.root_nanbox_f64(undefined());
    let mut parsed_args: Vec<Value> = Vec::new();
    let mut pending = false;
    let mut input = Value::Null;
    let mut request = None;
    let mut roots = Vec::new();
    let mut values = Vec::new();
    let outcome = crate::exception::catch_js_throw(|| {
        let interface = std::str::from_utf8(std::slice::from_raw_parts(interface, interface_len))
            .unwrap_or_else(|_| fail("Invalid component interface name"));
        let function = std::str::from_utf8(std::slice::from_raw_parts(function, function_len))
            .unwrap_or_else(|_| fail("Invalid component function name"));
        input = serde_json::from_slice(std::slice::from_raw_parts(args, args_len))
            .unwrap_or_else(|_| fail("Invalid component callback arguments"));
        request = input["$asyncRequest"].as_str().map(str::to_owned);
        parsed_args = (if request.is_some() {
            &input["args"]
        } else {
            &input
        })
        .as_array()
        .unwrap_or_else(|| fail("Invalid component callback arguments"))
        .clone();
        let imports = scope.root_nanbox_f64(context::imports(frame.owner.get_nanbox_f64()));
        let namespace = scope.root_nanbox_f64(if interface.is_empty() {
            imports.get_nanbox_f64()
        } else {
            field(imports.get_nanbox_f64(), interface.as_bytes())
        });
        let callback =
            scope.root_nanbox_f64(field(namespace.get_nanbox_f64(), function.as_bytes()));
        let context = context::get(frame.owner.get_nanbox_f64());
        let world = &(*context).world;
        let signature = context::signature(world, "imports", interface, function);
        if (signature["async"] == true) != request.is_some() {
            fail("Component import calling convention mismatch");
        }
        let params = signature["params"].as_array().unwrap();
        if params.len() != parsed_args.len() {
            fail("Component callback argument count mismatch");
        }
        lifetime.set_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::array::js_array_alloc_with_length(1) as i64,
        ));
        crate::array::js_array_set_f64(array_pointer(lifetime.get_nanbox_f64()).unwrap(), 0, 1.0);
        for (arg, param) in parsed_args.iter().zip(params) {
            roots.push(scope.root_nanbox_f64(value::decode_with_lifetime(
                arg,
                &param["ty"],
                world,
                frame.owner.get_nanbox_f64(),
                lifetime.get_nanbox_f64(),
            )));
        }
        values.extend(roots.iter().map(|root| root.get_nanbox_f64()));
        let result = crate::closure::js_native_call_value(
            callback.get_nanbox_f64(),
            crate::closure::plain_call_receiver(),
            values.as_ptr(),
            values.len(),
        );
        if let Some(request) = request.as_deref() {
            super::asynchronous::imports::register(
                frame.owner.get_nanbox_f64(),
                request,
                result,
                lifetime.get_nanbox_f64(),
            );
            pending = true;
            Vec::new()
        } else {
            serde_json::to_vec(&value::encode_for(result, frame.owner.get_nanbox_f64()))
                .expect("component value is JSON")
        }
    });
    if !pending {
        if let Some(array) = array_pointer(lifetime.get_nanbox_f64()) {
            crate::array::js_array_set_f64(array, 0, 0.0);
        }
    }
    drop((input, request));
    match outcome {
        Ok(bytes) => {
            reply(output, bytes);
            if pending {
                perry_abi::component_host::ASYNC_PENDING
            } else {
                perry_abi::component_host::OK
            }
        }
        Err(error) => {
            frame.capture(error);
            reply(output, b"JavaScript component import threw".to_vec());
            perry_abi::component_host::ERROR
        }
    }
}

/// Resource events use the live embedding frame without reentering its Store.
pub(super) unsafe extern "C" fn resource_invoke(
    token: u64,
    event: *const u8,
    event_len: usize,
    output: *mut Buffer,
) -> i32 {
    let frame = &*(token as usize as *const Frame<'_>);
    let mut parsed = Value::Null;
    let outcome = crate::exception::catch_js_throw(|| {
        parsed = serde_json::from_slice(std::slice::from_raw_parts(event, event_len))
            .unwrap_or_else(|_| fail("Invalid component resource event"));
        if parsed["kind"] == "drain" {
            let context = context::get(frame.owner.get_nanbox_f64());
            serde_json::to_vec(&std::mem::take(&mut (*context).pending_resource_drops)).unwrap()
        } else {
            super::host_resource::payload::event(frame.owner.get_nanbox_f64(), &parsed);
            b"[]".to_vec()
        }
    });
    drop(parsed);
    match outcome {
        Ok(bytes) => {
            reply(output, bytes);
            perry_abi::component_host::OK
        }
        Err(error) => {
            frame.capture(error);
            reply(output, b"JavaScript host resource event threw".to_vec());
            perry_abi::component_host::ERROR
        }
    }
}
