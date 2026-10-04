//! #10502 fallback-dispatch benchmark: bypass compiler method-site caches.
//! Build with `cargo build --profile perry-dev -p perry-runtime
//! --example method_dispatch_10502`; run with `<own-key count> <iterations>`.
//! Fit `perf stat -e instructions:u` at two iteration counts to remove startup.

use perry_runtime::{closure, gc::RuntimeHandleScope, object, value, JSValue};

extern "C" fn method(
    _closure: *const closure::ClosureHeader,
    _this: closure::JsThis,
    arg: f64,
) -> f64 {
    arg + 40.0
}

fn main() {
    let options: Vec<String> = std::env::args().collect();
    let width: usize = options[1].parse().unwrap();
    let count: usize = options[2].parse().unwrap();
    assert!((1..=65536).contains(&width));
    let scope = RuntimeHandleScope::new();
    let names: Vec<String> = (0..width).map(|i| format!("f{i:03}")).collect();
    let packed = names.join("\0");
    let recv = scope.root_nanbox_f64(value::js_nanbox_pointer(
        object::js_object_alloc_class_with_keys(
            0,
            width as u32,
            width as u32,
            packed.as_ptr(),
            packed.len() as u32,
        ) as i64,
    ));
    let callable = scope.root_nanbox_f64(value::js_nanbox_pointer(closure::js_closure_alloc(
        perry_runtime::fn_info!(method, 1; with_declared(1)),
        0,
    ) as i64));
    unsafe {
        object::js_object_set_field(
            JSValue::from_bits(recv.get_nanbox_u64()).as_pointer::<object::ObjectHeader>()
                as *mut _,
            (width - 1) as u32,
            JSValue::from_bits(callable.get_nanbox_u64()),
        );
        let name = names[width - 1].as_bytes();
        let mut checksum = 0.0;
        // Calls the dispatcher ABI directly so no compiler PIC can hide the scan.
        for i in 0..count {
            let arg = [(i & 1023) as f64];
            checksum += object::js_native_call_method(
                recv.get_nanbox_f64(),
                name.as_ptr().cast(),
                name.len(),
                arg.as_ptr(),
                1,
            );
        }
        println!("width={width} count={count} checksum={checksum}");
    }
}
