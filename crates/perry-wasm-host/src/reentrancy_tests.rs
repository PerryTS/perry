//! A JavaScript import may re-enter WebAssembly while the outer call holds the
//! shared store (Emscripten `invoke_*`/`dynCall`, imports calling `_malloc`).
//! Nested host access must route through the active import's `Caller`.
use super::*;
use std::cell::Cell;

/// `(module (import "env" "cb" (func $cb (result f64)))
///          (table $t (export "t") 1 funcref) (elem (i32.const 0) $inner)
///          (func (export "outer") (result f64) call $cb)
///          (func $inner (export "inner") (result f64) f64.const 7))`.
const REENTRANT_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7c, 0x02,
    0x0a, 0x01, 0x03, 0x65, 0x6e, 0x76, 0x02, 0x63, 0x62, 0x00, 0x00, 0x03, 0x03, 0x02, 0x00, 0x00,
    0x04, 0x04, 0x01, 0x70, 0x00, 0x01, 0x07, 0x15, 0x03, 0x01, 0x74, 0x01, 0x00, 0x05, 0x6f, 0x75,
    0x74, 0x65, 0x72, 0x00, 0x01, 0x05, 0x69, 0x6e, 0x6e, 0x65, 0x72, 0x00, 0x02, 0x09, 0x07, 0x01,
    0x00, 0x41, 0x00, 0x0b, 0x01, 0x02, 0x0a, 0x12, 0x02, 0x04, 0x00, 0x10, 0x00, 0x0b, 0x0b, 0x00,
    0x44, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1c, 0x40, 0x0b,
];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Nested {
    Export,
    TableFunction,
    StandaloneGlobal,
}

thread_local! {
    static INSTANCE: Cell<*mut WasmInstanceHandle> = const { Cell::new(std::ptr::null_mut()) };
    static MODE: Cell<Nested> = const { Cell::new(Nested::Export) };
    static NESTED: Cell<Option<f64>> = const { Cell::new(None) };
}

unsafe fn nested_table_function_call(inst: *mut WasmInstanceHandle) -> Option<f64> {
    let table = externals::perry_wasm_host_instance_export_extern(inst, c"t".as_ptr(), 1);
    let mut bits = 0u64;
    let mut is_null = 0i32;
    let mut function = std::ptr::null_mut();
    let got =
        externals::perry_wasm_host_table_get(table, 0, &mut bits, &mut is_null, &mut function);
    perry_wasm_host_extern_drop(table);
    if got == 0 || is_null != 0 || function.is_null() {
        return None;
    }
    let mut out_kind = 0u8;
    let mut out_bits = 0u64;
    let mut out_count = 0usize;
    let mut err = std::ptr::null_mut();
    // Non-null empty argument buffers, as the runtime's caller passes them.
    let no_kinds = [0u8; 1];
    let no_bits = [0u64; 1];
    let called = externals::perry_wasm_host_func_call(
        function,
        no_kinds.as_ptr(),
        no_bits.as_ptr(),
        0,
        &mut out_kind,
        &mut out_bits,
        1,
        &mut out_count,
        &mut err,
    );
    perry_wasm_host_extern_drop(function);
    (called != 0 && out_count == 1 && out_kind == WASM_VAL_KIND_F64)
        .then(|| f64::from_bits(out_bits))
}

unsafe extern "C" fn reenters_wasm(
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
    assert_eq!(result_count, 1);
    let inst = INSTANCE.with(Cell::get);
    let nested = match MODE.with(Cell::get) {
        Nested::Export => match call_export(&mut *inst, "inner", &[]) {
            Ok(values) => match values.as_slice() {
                [WasmVal::F64(value)] => Some(*value),
                _ => None,
            },
            Err(_) => None,
        },
        Nested::TableFunction => nested_table_function_call(inst),
        Nested::StandaloneGlobal => {
            let global =
                externals::perry_wasm_host_global_new(WASM_VAL_KIND_F64, 0, 5.0f64.to_bits());
            let mut kind = 0u8;
            let mut bits = 0u64;
            let read = !global.is_null()
                && externals::perry_wasm_host_global_get(global, &mut kind, &mut bits) != 0;
            perry_wasm_host_extern_drop(global);
            read.then(|| f64::from_bits(bits))
        }
    };
    NESTED.with(|slot| slot.set(nested));
    *result_bits = (nested.unwrap_or(-100.0) + 1.0).to_bits();
    1
}

fn outer_call_with(mode: Nested) -> (Option<f64>, Vec<WasmVal>) {
    let module = compile(REENTRANT_WASM).expect("compile re-entrant module");
    let mut instance = instantiate_with_import_callback(&module, Some(reenters_wasm), 0)
        .expect("instantiate re-entrant module");
    // Resolve the nested export first: the export cache must not grow while
    // the outer call borrows its entry.
    assert_eq!(
        call_export(&mut instance, "inner", &[]).expect("warm inner"),
        [WasmVal::F64(7.0)]
    );
    INSTANCE.with(|slot| slot.set(&mut instance as *mut WasmInstanceHandle));
    MODE.with(|slot| slot.set(mode));
    NESTED.with(|slot| slot.set(None));
    let outer = call_export(&mut instance, "outer", &[]).expect("outer call");
    INSTANCE.with(|slot| slot.set(std::ptr::null_mut()));
    (NESTED.with(Cell::get), outer)
}

#[test]
fn import_callback_can_call_another_export() {
    let (nested, outer) = outer_call_with(Nested::Export);
    assert_eq!(nested, Some(7.0));
    assert_eq!(outer, [WasmVal::F64(8.0)]);
}

#[test]
fn import_callback_can_call_a_funcref_table_function() {
    let (nested, outer) = outer_call_with(Nested::TableFunction);
    assert_eq!(nested, Some(7.0));
    assert_eq!(outer, [WasmVal::F64(8.0)]);
}

#[test]
fn import_callback_can_use_standalone_externals() {
    let (nested, outer) = outer_call_with(Nested::StandaloneGlobal);
    assert_eq!(nested, Some(5.0));
    assert_eq!(outer, [WasmVal::F64(6.0)]);
    // The outermost store is free again once the call returns.
    let global = externals::perry_wasm_host_global_new(WASM_VAL_KIND_I32, 1, 0);
    assert!(
        !global.is_null(),
        "the store must be released after the call"
    );
    perry_wasm_host_extern_drop(global);
}

#[test]
fn overlapping_access_without_an_import_boundary_is_refused() {
    let nested =
        host_runtime::with_host_runtime(|_| host_runtime::with_host_runtime(|_| ()).is_none());
    assert_eq!(nested, Some(true));
    assert!(host_runtime::with_host_runtime(|_| ()).is_some());
}
