//! One-shot native command entry point. No JS callback or heap pointer crosses
//! the optional engine boundary, and every native allocation outlives the catch.
use super::*;

#[no_mangle]
pub extern "C" fn js_component_run_command(bytes: f64, options: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let bytes = scope.root_nanbox_f64(bytes);
    let options_input = scope.root_nanbox_f64(options);
    let mut wasm = Vec::new();
    let mut options_value = Value::Null;
    let mut options = Vec::new();
    let mut output = Vec::new();
    let result = crate::exception::catch_js_throw(|| {
        let ops = host::ops();
        options_value =
            if JSValue::from_bits(options_input.get_nanbox_f64().to_bits()).is_undefined() {
                serde_json::json!({})
            } else {
                value::encode(options_input.get_nanbox_f64())
            };
        options = serde_json::to_vec(&options_value).unwrap();
        wasm = crate::buffer::bytes::no_gc(|no_gc| {
            crate::buffer::bytes::bytes(bytes.get_nanbox_f64(), no_gc).map(<[u8]>::to_vec)
        })
        .unwrap_or_else(|_| fail("Expected command component bytes"));
        let mut buffer = Buffer::default();
        let status = unsafe {
            (ops.perry_component_host_run_command)(
                wasm.as_ptr(),
                wasm.len(),
                options.as_ptr(),
                options.len(),
                &mut buffer,
            )
        };
        output = unsafe { take(buffer) };
        if status != perry_abi::component_host::OK {
            fail(&String::from_utf8_lossy(&output));
        }
        let code: i32 = serde_json::from_slice(&output)
            .unwrap_or_else(|_| fail("Invalid command exit status from component host"));
        code as f64
    });
    drop((wasm, options_value, options, output));
    finish(result)
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_RUN_COMMAND: extern "C" fn(f64, f64) -> f64 = js_component_run_command;
