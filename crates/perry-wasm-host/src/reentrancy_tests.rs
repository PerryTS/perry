use super::*;

unsafe extern "C" fn tries_nested_store_access(
    _context: u64,
    _module: *const u8,
    _module_len: usize,
    _name: *const u8,
    _name_len: usize,
    _arg_kinds: *const u8,
    _arg_bits: *const u64,
    _arg_count: usize,
    _result_kinds: *const u8,
    result_bits: *mut u64,
    result_count: usize,
) -> i32 {
    assert!(
        externals::perry_wasm_host_global_new(WASM_VAL_KIND_I32, 1, 0).is_null(),
        "nested access must fail before borrowing the active store again"
    );
    assert_eq!(result_count, 1);
    *result_bits = 9.5f64.to_bits();
    1
}

#[test]
fn javascript_import_callback_cannot_reborrow_the_shared_store() {
    let module = compile(super::tests::IMPORT_F64_RESULT_WASM).expect("compile import module");
    let mut instance =
        instantiate_with_import_callback(&module, Some(tries_nested_store_access), 0)
            .expect("instantiate import module");
    assert_eq!(
        call_export(&mut instance, "call", &[]).expect("outer wasm call remains usable"),
        [WasmVal::F64(9.5)]
    );

    let handle = externals::perry_wasm_host_global_new(WASM_VAL_KIND_I32, 1, 0);
    assert!(
        !handle.is_null(),
        "the borrow guard must clear after the call"
    );
    perry_wasm_host_extern_drop(handle);
}
