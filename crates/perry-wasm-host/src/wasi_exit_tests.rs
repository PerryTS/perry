use super::*;
unsafe extern "C" fn exit_callback(
    context: u64,
    _: *const u8,
    _: usize,
    _: *const u8,
    _: usize,
    _: *const u8,
    args: *const u64,
    _: usize,
    _: *const u8,
    results: *mut u64,
    _: usize,
) -> i32 {
    if context == 1 {
        return 1;
    } // A user-defined JS override returns normally.
    *results = *args;
    perry_abi::wasi::IMPORT_EXIT
}
#[test]
fn exit_unwinds_before_next_instruction_and_preserves_unsigned_status() {
    for code in [0u32, 7, 255, 0x80000000, u32::MAX] {
        let wasm = wat::parse_str(format!(
            r#"(module
            (import "wasi_snapshot_preview1" "proc_exit" (func $exit (param i32)))
            (global $after (mut i32) (i32.const 0))
            (func (export "_start") i32.const {} call $exit i32.const 1 global.set $after)
            (func (export "after") (result i32) global.get $after))"#,
            code as i32
        ))
        .unwrap();
        let module = compile(&wasm).unwrap();
        let mut instance =
            instantiate_with_import_callback(&module, Some(exit_callback), 0).unwrap();
        call_export(&mut instance, "_start", &[]).unwrap();
        let mut status = 0;
        assert_eq!(
            perry_wasm_host_instance_take_exit_code(&mut instance, &mut status),
            1
        );
        assert_eq!(status as u32, code);
        assert_eq!(
            call_export(&mut instance, "after", &[]).unwrap(),
            vec![WasmVal::I32(0)]
        );
        assert_eq!(
            perry_wasm_host_instance_take_exit_code(&mut instance, &mut status),
            0
        );
        let mut overridden =
            instantiate_with_import_callback(&module, Some(exit_callback), 1).unwrap();
        call_export(&mut overridden, "_start", &[]).unwrap();
        assert_eq!(
            call_export(&mut overridden, "after", &[]).unwrap(),
            vec![WasmVal::I32(1)]
        );
        assert_eq!(
            perry_wasm_host_instance_take_exit_code(&mut overridden, &mut status),
            0
        );
    }
}
