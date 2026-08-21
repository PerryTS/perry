//! Same-thread native-to-JS callback trampolines for `bun:ffi`.
//!
//! A callback pointer must carry both an arbitrary C scalar signature and a
//! Perry callback identity. The hosted ABIs already split scalar arguments
//! into integer and floating-point register files, so each static trampoline
//! places its slot number in a scratch register and branches to one assembly
//! entry point. That entry saves both register files, calls the Rust dispatcher,
//! and places the returned bits in both the integer and FP return registers.
//!
//! The pool is intentionally finite and never reuses a closed slot. Reusing a
//! pointer after `close()` could silently invoke an unrelated later callback;
//! retaining a poisoned slot instead makes such a stale native call return zero.

use super::call::{self, MAX_ARGS, MAX_FLOAT_ARGS};
use super::types::*;
use crate::closure::ClosureHeader;
use crate::value::JSValue;
use std::sync::Mutex;
use std::thread::ThreadId;

const MAX_CALLBACKS: usize = 128;
#[cfg(target_arch = "x86_64")]
const MAX_CALLBACK_INT_ARGS: usize = 6;
#[cfg(target_arch = "aarch64")]
const MAX_CALLBACK_INT_ARGS: usize = 8;
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
const MAX_CALLBACK_INT_ARGS: usize = 0;

#[derive(Clone)]
struct CallbackRecord {
    callback_bits: u64,
    owner: ThreadId,
    ret: u8,
    argc: u8,
    args: [u8; MAX_ARGS],
    open: bool,
}

static CALLBACKS: Mutex<Vec<CallbackRecord>> = Mutex::new(Vec::new());

#[cfg(target_vendor = "apple")]
macro_rules! callback_symbol {
    ($name:literal) => {
        concat!("_", $name)
    };
}
#[cfg(not(target_vendor = "apple"))]
macro_rules! callback_symbol {
    ($name:literal) => {
        $name
    };
}

#[cfg(all(unix, target_arch = "aarch64"))]
const CALLBACK_TRAMPOLINE_SIZE: usize = 8;
#[cfg(all(unix, target_arch = "x86_64"))]
const CALLBACK_TRAMPOLINE_SIZE: usize = 11;

#[cfg(all(unix, target_arch = "aarch64"))]
core::arch::global_asm!(
    ".text",
    ".p2align 2",
    concat!(
        ".globl ",
        callback_symbol!("perry_ffi_callback_trampoline_base")
    ),
    concat!(callback_symbol!("perry_ffi_callback_trampoline_base"), ":"),
    ".set Lperry_ffi_callback_index, 0",
    ".rept 128",
    // Every AArch64 instruction is four bytes: mov-immediate + branch gives
    // one fixed eight-byte slot whose address can be derived from the base.
    "    mov x16, #Lperry_ffi_callback_index",
    "    b Lperry_ffi_callback_entry",
    ".set Lperry_ffi_callback_index, Lperry_ffi_callback_index + 1",
    ".endr",
    "Lperry_ffi_callback_entry:",
    // 8 GPR slots, 8 FP slots, and the link register. Keep SP 16-aligned.
    "    sub sp, sp, #144",
    "    stp x0, x1, [sp, #0]",
    "    stp x2, x3, [sp, #16]",
    "    stp x4, x5, [sp, #32]",
    "    stp x6, x7, [sp, #48]",
    "    stp d0, d1, [sp, #64]",
    "    stp d2, d3, [sp, #80]",
    "    stp d4, d5, [sp, #96]",
    "    stp d6, d7, [sp, #112]",
    "    str x30, [sp, #128]",
    "    mov x0, x16",
    "    mov x1, sp",
    "    add x2, sp, #64",
    concat!("    bl ", callback_symbol!("perry_ffi_callback_dispatch")),
    // Publish the same raw bits in x0 and d0; the declared return type decides
    // which register the native caller observes (s0 reads d0's low 32 bits).
    "    fmov d0, x0",
    "    ldr x30, [sp, #128]",
    "    add sp, sp, #144",
    "    ret",
);

#[cfg(all(unix, target_arch = "x86_64"))]
core::arch::global_asm!(
    ".text",
    ".p2align 4, 0x90",
    concat!(
        ".globl ",
        callback_symbol!("perry_ffi_callback_trampoline_base")
    ),
    concat!(callback_symbol!("perry_ffi_callback_trampoline_base"), ":"),
    ".set Lperry_ffi_callback_index, 0",
    ".rept 128",
    // Encode a fixed-width `mov r10d, imm32; jmp rel32` slot explicitly.
    // An assembler-selected short jump would make pointer arithmetic invalid.
    "    .byte 0x41, 0xba",
    "    .long Lperry_ffi_callback_index",
    "    .byte 0xe9",
    "    .long Lperry_ffi_callback_entry - . - 4",
    ".set Lperry_ffi_callback_index, Lperry_ffi_callback_index + 1",
    ".endr",
    "Lperry_ffi_callback_entry:",
    // SysV has 6 integer and 8 FP argument registers. Entry RSP is 8 mod 16;
    // a 120-byte frame restores 16-byte alignment before the Rust call.
    "    sub rsp, 120",
    "    mov qword ptr [rsp + 0], rdi",
    "    mov qword ptr [rsp + 8], rsi",
    "    mov qword ptr [rsp + 16], rdx",
    "    mov qword ptr [rsp + 24], rcx",
    "    mov qword ptr [rsp + 32], r8",
    "    mov qword ptr [rsp + 40], r9",
    "    movsd qword ptr [rsp + 48], xmm0",
    "    movsd qword ptr [rsp + 56], xmm1",
    "    movsd qword ptr [rsp + 64], xmm2",
    "    movsd qword ptr [rsp + 72], xmm3",
    "    movsd qword ptr [rsp + 80], xmm4",
    "    movsd qword ptr [rsp + 88], xmm5",
    "    movsd qword ptr [rsp + 96], xmm6",
    "    movsd qword ptr [rsp + 104], xmm7",
    "    mov rdi, r10",
    "    lea rsi, [rsp]",
    "    lea rdx, [rsp + 48]",
    concat!("    call ", callback_symbol!("perry_ffi_callback_dispatch")),
    "    movq xmm0, rax",
    "    add rsp, 120",
    "    ret",
);

#[cfg(all(unix, any(target_arch = "x86_64", target_arch = "aarch64")))]
extern "C" {
    static perry_ffi_callback_trampoline_base: u8;
}

fn trampoline_ptr(index: usize) -> Option<usize> {
    #[cfg(all(unix, any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        if index < MAX_CALLBACKS {
            // Each assembly slot has a fixed width, so no relocated pointer
            // table (and no text relocations) is needed.
            let base = core::ptr::addr_of!(perry_ffi_callback_trampoline_base) as usize;
            return Some(base + index * CALLBACK_TRAMPOLINE_SIZE);
        }
    }
    let _ = index;
    None
}

unsafe fn object_ptr(value: f64) -> Option<*mut crate::object::ObjectHeader> {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let address = crate::value::js_nanbox_get_pointer(value) as usize;
    (address != 0).then_some(address as *mut crate::object::ObjectHeader)
}

unsafe fn get_field(object: *mut crate::object::ObjectHeader, name: &str) -> f64 {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    f64::from_bits(crate::object::js_object_get_field_by_name(object, key).bits())
}

fn set_field(object: *mut crate::object::ObjectHeader, name: &str, value: f64) {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::object::js_object_set_field_by_name(object, key, value);
}

fn throw_type(message: &str) -> ! {
    crate::fs::validate::throw_type_error_with_code(message, "ERR_INVALID_ARG_TYPE")
}

fn parse_signature(definition: f64) -> (u8, u8, [u8; MAX_ARGS]) {
    let Some(object) = (unsafe { object_ptr(definition) }) else {
        throw_type("JSCallback callback definition must be an object");
    };

    let threadsafe = unsafe { get_field(object, "threadsafe") };
    if !JSValue::from_bits(threadsafe.to_bits()).is_undefined()
        && crate::value::js_is_truthy(threadsafe) != 0
    {
        crate::fs::validate::throw_error_with_code(
            "bun:ffi: threadsafe JSCallback is not supported; callbacks must be invoked on their creating thread",
            "ERR_NOT_IMPLEMENTED",
        );
    }

    let mut args = [T_VOID; MAX_ARGS];
    let args_value = unsafe { get_field(object, "args") };
    let args_jv = JSValue::from_bits(args_value.to_bits());
    let mut argc = 0usize;
    if !args_jv.is_undefined() && !args_jv.is_null() {
        if !JSValue::from_bits(crate::array::js_array_is_array(args_value).to_bits()).as_bool() {
            throw_type("JSCallback definition.args must be an array");
        }
        let array = crate::value::js_nanbox_get_pointer(args_value) as usize
            as *const crate::array::ArrayHeader;
        let len = crate::array::js_array_length(array) as usize;
        if len > MAX_ARGS {
            throw_type(&format!("JSCallback supports at most {MAX_ARGS} arguments"));
        }
        for (index, slot) in args.iter_mut().take(len).enumerate() {
            let value = crate::array::js_array_get(array, index as u32);
            *slot = unsafe { super::types::parse_ffi_type_checked(f64::from_bits(value.bits())) }
                .unwrap_or_else(|message| throw_type(&format!("JSCallback: {message}")));
        }
        argc = len;
    }

    let returns = unsafe { get_field(object, "returns") };
    let returns_jv = JSValue::from_bits(returns.to_bits());
    let ret = if returns_jv.is_undefined() || returns_jv.is_null() {
        T_VOID
    } else {
        unsafe { super::types::parse_ffi_type_checked(returns) }
            .unwrap_or_else(|message| throw_type(&format!("JSCallback: {message}")))
    };

    let mut ints = 0usize;
    let mut floats = 0usize;
    for &ty in &args[..argc] {
        match ty {
            T_VOID => throw_type("JSCallback: void is not a valid argument type"),
            T_NAPI_ENV | T_NAPI_VALUE | T_BUFFER => {
                throw_type("JSCallback: napi and buffer argument types are not supported")
            }
            ty if super::types::is_float_class(ty) => floats += 1,
            _ => ints += 1,
        }
    }
    if ints > MAX_CALLBACK_INT_ARGS {
        throw_type(&format!(
            "JSCallback supports at most {MAX_CALLBACK_INT_ARGS} integer/pointer register arguments on this target"
        ));
    }
    if floats > MAX_FLOAT_ARGS {
        throw_type(&format!(
            "JSCallback supports at most {MAX_FLOAT_ARGS} floating-point register arguments"
        ));
    }
    match ret {
        T_NAPI_ENV | T_NAPI_VALUE | T_BUFFER => {
            throw_type("JSCallback: napi and buffer return types are not supported")
        }
        T_CSTRING => {
            throw_type("JSCallback: cstring returns are not supported; return ptr instead")
        }
        _ => {}
    }

    (ret, argc as u8, args)
}

extern "C" fn callback_close_thunk(closure: *const ClosureHeader) -> f64 {
    let index = crate::closure::js_closure_get_capture_bits(closure, 0) as usize;
    if let Some(record) = CALLBACKS.lock().unwrap().get_mut(index) {
        record.open = false;
        record.callback_bits = crate::value::TAG_UNDEFINED;
    }
    super::undefined()
}

fn close_closure(index: usize) -> f64 {
    let function = callback_close_thunk as *const u8;
    crate::closure::js_register_closure_arity(function, 0);
    crate::closure::js_register_closure_length(function, 0);
    let closure = crate::closure::js_closure_alloc(function, 1);
    crate::closure::js_closure_set_capture_bits(closure, 0, index as u64);
    crate::object::set_bound_native_closure_name(closure, "close");
    crate::object::set_builtin_closure_length(closure as usize, 0);
    crate::value::js_nanbox_pointer(closure as i64)
}

/// Construct a Bun-shaped `{ ptr, threadsafe, close }` callback wrapper.
pub(crate) fn js_callback_value(callback: f64, definition: f64) -> f64 {
    if !call::platform_supported() {
        crate::fs::validate::throw_error_with_code(
            "bun:ffi JSCallback is supported only on unix x86_64 / aarch64",
            "ERR_NOT_IMPLEMENTED",
        );
    }
    if !crate::object::value_is_callable(callback) {
        throw_type("JSCallback expects a function as its first argument");
    }
    let (ret, argc, args) = parse_signature(definition);

    let index = {
        let mut callbacks = CALLBACKS.lock().unwrap();
        if callbacks.len() >= MAX_CALLBACKS {
            crate::fs::validate::throw_error_with_code(
                &format!("bun:ffi JSCallback exhausted its {MAX_CALLBACKS}-slot process pool"),
                "ERR_OUT_OF_RANGE",
            );
        }
        let index = callbacks.len();
        callbacks.push(CallbackRecord {
            callback_bits: callback.to_bits(),
            owner: std::thread::current().id(),
            ret,
            argc,
            args,
            open: true,
        });
        index
    };
    let pointer = trampoline_ptr(index).expect("supported callback target has trampoline table");

    // The registry roots callback before any of the following JS allocations.
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = crate::object::js_object_alloc(0, 3);
    let object = scope.root_raw_mut_ptr(object);
    set_field(
        object.get_raw_mut_ptr::<crate::object::ObjectHeader>(),
        "ptr",
        super::number_value(pointer as f64),
    );
    set_field(
        object.get_raw_mut_ptr::<crate::object::ObjectHeader>(),
        "threadsafe",
        f64::from_bits(crate::value::TAG_FALSE),
    );
    let close = scope.root_nanbox_f64(close_closure(index));
    set_field(
        object.get_raw_mut_ptr::<crate::object::ObjectHeader>(),
        "close",
        close.get_nanbox_f64(),
    );
    f64::from_bits(
        JSValue::object_ptr(object.get_raw_mut_ptr::<crate::object::ObjectHeader>() as *mut u8)
            .bits(),
    )
}

unsafe fn native_arg_value(ty: u8, bits: u64) -> f64 {
    match ty {
        T_F64 => super::number_value(f64::from_bits(bits)),
        T_F32 => super::number_value(f32::from_bits(bits as u32) as f64),
        _ => call::convert_int_return(ty, bits),
    }
}

unsafe fn native_return_bits(ty: u8, value: f64) -> u64 {
    match ty {
        T_VOID => 0,
        T_F64 => call::value_to_f64_num(value).to_bits(),
        T_F32 => (call::value_to_f64_num(value) as f32).to_bits() as u64,
        T_BOOL => crate::value::js_is_truthy(value) as u64,
        T_PTR | T_FUNCTION => call::value_to_pointer_arg(value) as u64,
        _ => call::value_to_u64_int(value),
    }
}

/// Assembly callback target. Never unwinds across native code: an uncaught JS
/// exception is trapped and converted to the declared ABI's zero value. This
/// matches the module's fail-closed same-thread contract and, critically,
/// never sends a Perry unwind through an arbitrary third-party C frame.
#[no_mangle]
pub unsafe extern "C" fn perry_ffi_callback_dispatch(
    index: usize,
    integer_registers: *const usize,
    float_registers: *const u64,
) -> u64 {
    let record = {
        let callbacks = CALLBACKS.lock().unwrap();
        let Some(record) = callbacks.get(index) else {
            return 0;
        };
        if !record.open || record.owner != std::thread::current().id() {
            return 0;
        }
        record.clone()
    };

    let scope = crate::gc::RuntimeHandleScope::new();
    let callback = scope.root_nanbox_f64(f64::from_bits(record.callback_bits));
    let mut argument_roots = Vec::with_capacity(record.argc as usize);
    let mut integer_index = 0usize;
    let mut float_index = 0usize;
    for &ty in &record.args[..record.argc as usize] {
        let bits = if super::types::is_float_class(ty) {
            let bits = *float_registers.add(float_index);
            float_index += 1;
            bits
        } else {
            let bits = *integer_registers.add(integer_index) as u64;
            integer_index += 1;
            bits
        };
        argument_roots.push(scope.root_nanbox_f64(native_arg_value(ty, bits)));
    }
    let arguments: Vec<f64> = argument_roots
        .iter()
        .map(|handle| handle.get_nanbox_f64())
        .collect();

    match crate::exception::js_call_catching(|| {
        crate::closure::js_native_call_value(
            callback.get_nanbox_f64(),
            arguments.as_ptr(),
            arguments.len(),
        )
    }) {
        Ok(value) => native_return_bits(record.ret, value),
        Err(_error) => 0,
    }
}

pub(crate) fn scan_callback_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let owner = std::thread::current().id();
    for record in CALLBACKS.lock().unwrap().iter_mut() {
        if record.open && record.owner == owner {
            visitor.visit_nanbox_u64_slot(&mut record.callback_bits);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_pool_has_stable_nonzero_trampolines() {
        if call::platform_supported() {
            let first = trampoline_ptr(0).unwrap();
            let last = trampoline_ptr(MAX_CALLBACKS - 1).unwrap();
            assert_ne!(first, 0);
            assert_ne!(last, 0);
            assert_ne!(first, last);
        }
    }
}
