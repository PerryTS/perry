use super::*;
use crate::gc::RuntimeHandle;
use std::cell::Cell;

pub(super) struct Frame<'a> {
    pub owner: RuntimeHandle<'a>,
    pub error: RuntimeHandle<'a>,
    pub threw: Cell<bool>,
}

impl<'a> Frame<'a> {
    pub fn new(scope: &'a RuntimeHandleScope, owner: f64) -> Self {
        Self {
            owner: scope.root_nanbox_f64(owner),
            error: scope.root_nanbox_f64(undefined()),
            threw: Cell::new(false),
        }
    }
    pub fn token(&self) -> u64 {
        self as *const Self as usize as u64
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
    let mut parsed_args: Vec<Value> = Vec::new();
    let mut roots = Vec::new();
    let mut values = Vec::new();
    let outcome = crate::exception::catch_js_throw(|| {
        let interface = std::str::from_utf8(std::slice::from_raw_parts(interface, interface_len))
            .unwrap_or_else(|_| fail("Invalid component interface name"));
        let function = std::str::from_utf8(std::slice::from_raw_parts(function, function_len))
            .unwrap_or_else(|_| fail("Invalid component function name"));
        parsed_args = serde_json::from_slice(std::slice::from_raw_parts(args, args_len))
            .unwrap_or_else(|_| fail("Invalid component callback arguments"));
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
        let params = signature["params"].as_array().unwrap();
        if params.len() != parsed_args.len() {
            fail("Component callback argument count mismatch");
        }
        for (arg, param) in parsed_args.iter().zip(params) {
            roots.push(scope.root_nanbox_f64(value::decode(
                arg,
                &param["ty"],
                world,
                frame.owner.get_nanbox_f64(),
            )));
        }
        values.extend(roots.iter().map(|root| root.get_nanbox_f64()));
        let result = crate::closure::js_native_call_value(
            callback.get_nanbox_f64(),
            crate::closure::plain_call_receiver(),
            values.as_ptr(),
            values.len(),
        );
        serde_json::to_vec(&value::encode(result)).expect("component value is JSON")
    });
    match outcome {
        Ok(bytes) => {
            reply(output, bytes);
            perry_abi::component_host::OK
        }
        Err(error) => {
            frame.error.set_nanbox_f64(error);
            frame.threw.set(true);
            reply(output, b"JavaScript component import threw".to_vec());
            perry_abi::component_host::ERROR
        }
    }
}
