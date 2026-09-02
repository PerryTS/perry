//! Arbitrary-arity all-`f64` call trampoline.
//!
//! Perry-generated methods and constructors have the C signature
//! `double(double this, double arg0, …, double argN)`. The dynamic vtable
//! dispatch (`object::class_registry::dispatch::call_vtable_method`) must invoke
//! such a function for an arity only known at runtime — and a synthesized
//! capture-stashing constructor can have 130+ params (one per captured
//! enclosing local in a giant minified bundle, e.g. Next.js app-route-turbo's
//! route-module class `rJ`). Hand-writing a `match`-arm-per-arity dispatch caps
//! out (the pre-#5437 64-arm cap silently transmuted a 135-param ctor to a
//! 64-arg signature in release builds, so every param past the 64th read
//! register/stack garbage — a captured function arrived non-callable and the
//! ctor threw "value is not a function").
//!
//! Because EVERY argument is an `f64`, the platform C ABI is fully determined:
//! the first 8 floating-point args go in FP argument registers and the rest are
//! spilled to a 16-byte-aligned stack area. This module implements that call
//! directly in assembly for the two hosted architectures (aarch64 + x86-64) —
//! as naked functions that carry their own frame pointer and unwind
//! description, because the unwinder has to step through them while the
//! callee runs (#9446; see the comment above the trampolines). Other targets
//! fall back to a fixed-arity dispatch good to 16 args (no Perry target other
//! than the two asm ones exercises high-arity dynamic ctor dispatch today).

/// Call `func_ptr` (a `extern "C" double(double, …)` with `args.len()` f64
/// params) passing every element of `args` as an f64 argument. Returns the f64
/// result.
///
/// # Safety
/// `func_ptr` must be a valid code pointer to a function whose C signature is
/// `double(double × args.len())`. All Perry method/ctor params are `f64`.
#[inline]
pub(crate) unsafe fn call_all_f64(func_ptr: usize, args: &[f64]) -> f64 {
    #[cfg(target_arch = "aarch64")]
    {
        let (reg, stacked, stack_bytes) = split_register_and_stacked(args);
        call_all_f64_aarch64(
            func_ptr,
            reg.as_ptr(),
            stacked.as_ptr(),
            stacked.len(),
            stack_bytes,
        )
    }
    // NOTE: gated to NON-Windows x86-64. The asm below is the SysV ABI (FP args
    // in xmm0..xmm7, no shadow space). The Windows x64 ABI passes FP args in
    // xmm0..xmm3 and requires a 32-byte shadow space, so the SysV asm would
    // mis-pass 5+ args. Win64 falls through to the portable fallback instead.
    #[cfg(all(target_arch = "x86_64", not(target_os = "windows")))]
    {
        let (reg, stacked, stack_bytes) = split_register_and_stacked(args);
        call_all_f64_x86_64(
            func_ptr,
            reg.as_ptr(),
            stacked.as_ptr(),
            stacked.len(),
            stack_bytes,
        )
    }
    #[cfg(not(any(
        target_arch = "aarch64",
        all(target_arch = "x86_64", not(target_os = "windows"))
    )))]
    {
        call_all_f64_fallback(func_ptr, args)
    }
}

/// Both asm ABIs (AAPCS64 and SysV x86-64) agree on the split: the first eight
/// f64 args ride in FP argument registers, the rest are spilled to the stack in
/// order, eight bytes each, with the stack pointer 16-byte aligned at the call.
/// Register slots past `args.len()` are padded with `0.0` — the callee has no
/// such parameter and never reads them.
#[cfg(any(
    target_arch = "aarch64",
    all(target_arch = "x86_64", not(target_os = "windows"))
))]
#[inline]
fn split_register_and_stacked(args: &[f64]) -> ([f64; 8], &[f64], usize) {
    let mut reg = [0.0f64; 8];
    let in_regs = args.len().min(reg.len());
    reg[..in_regs].copy_from_slice(&args[..in_regs]);
    let stacked = if args.len() > reg.len() {
        &args[reg.len()..]
    } else {
        &[][..]
    };
    // A 16-byte multiple keeps the stack aligned across the call.
    let stack_bytes = (stacked.len() * 8 + 15) & !15;
    (reg, stacked, stack_bytes)
}

// ---------------------------------------------------------------------------
// The two asm trampolines are NAKED functions that set up their own frame
// pointer and describe it to the unwinder, rather than inline `asm!` blocks
// inside an ordinary Rust function. That is the whole point of their shape,
// and the reason is #9446.
//
// The trampoline has to lower the stack pointer by a runtime-computed amount
// (the spilled-argument area) and leave it there ACROSS the call. Inside an
// `asm!` block the compiler does not know that, so the frame description it
// emits for the surrounding function still says "CFA = rsp + 32" (measured on
// x86-64 Linux, where LLVM keeps no frame pointer) while rsp is really
// `stack_bytes` lower. Every unwinder that steps through the trampoline while
// the callee runs — the GC's native-root walk (`gc/roots/stack_maps.rs`, an
// `_Unwind_Backtrace`), the exception transport (`_Unwind_RaiseException`),
// gdb — then reads the trampoline's return address from the wrong slot. What it
// finds there is a spilled argument or a saved register, so the walk either
// stops (silently dropping every frame ABOVE the trampoline: the collector
// never sees those frames' roots and frees or moves what they hold) or, when
// the garbage is not a mapped address, faults inside libgcc's fallback frame
// probe. The second shape is `PERRY_GC_SCHEDULE_SEED=1 PERRY_GC_SCHEDULE_RATE=1`
// on the Claude Code bundle at safepoint 4266: a minor at a loop poll inside a
// dynamically constructed class whose synthesized constructor takes more than
// eight params. aarch64 never showed it only because LLVM happened to keep a
// frame pointer there, so the CFA was `x29`-relative and immune to `sp`.
//
// With a naked function the frame IS the code below: `rbp`/`x29` is set from
// the entry stack pointer before anything moves, the CFA is defined off that
// register, and the callee restores it like any callee-saved register. The
// dynamic `rsp`/`sp` adjustment is then invisible to unwinding, on every
// target and under every frame-pointer setting, and the frame record is also
// exactly what a frame-pointer chain walk expects.
// `tests::the_unwinder_steps_through_a_trampoline_with_stacked_args` pins the
// contract from inside a stacked-argument callee.
//
// The one target that keeps the inline-`asm!` shape is Windows ARM64, where
// unwinding is SEH and its metadata comes from the compiler's own prologue —
// see `call_all_f64_aarch64`'s Windows twin below.
// ---------------------------------------------------------------------------

/// The unwind directives inside the naked trampolines: DWARF CFI, which is what
/// the Itanium unwinder — the GC's native-root walk and the exception transport
/// on every ELF and Mach-O host — reads.
#[cfg(all(
    not(target_os = "windows"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]
macro_rules! cfi {
    ($directive:literal) => {
        $directive
    };
}

/// AAPCS64: `x0` = callee, `x1` = the eight register args, `x2`/`x3` = the
/// spilled args and their count, `x4` = the 16-byte-rounded spill area size.
/// Register args go in `d0`–`d7`; args 9+ are copied to `[sp + i*8]` in order.
#[cfg(all(target_arch = "aarch64", not(target_os = "windows")))]
#[unsafe(naked)]
unsafe extern "C" fn call_all_f64_aarch64(
    func_ptr: usize,
    reg: *const f64,
    stacked: *const f64,
    stacked_count: usize,
    stack_bytes: usize,
) -> f64 {
    core::arch::naked_asm!(
        // Frame record first, before sp moves by a runtime amount.
        // Naked functions get no CFI region from rustc; this body is its own.
        cfi!(".cfi_startproc"),
        "stp x29, x30, [sp, #-16]!",
        cfi!(".cfi_def_cfa_offset 16"),
        cfi!(".cfi_offset w30, -8"),
        cfi!(".cfi_offset w29, -16"),
        "mov x29, sp",
        cfi!(".cfi_def_cfa w29, 16"),
        // Reserve the (16-byte multiple) spill area and copy args 9+ into it.
        "sub sp, sp, x4",
        "mov x9, xzr",
        "cbz x3, 3f",
        "2:",
        "ldr x10, [x2, x9, lsl #3]",
        "str x10, [sp, x9, lsl #3]",
        "add x9, x9, #1",
        "cmp x9, x3",
        "b.lo 2b",
        "3:",
        // FP argument registers, loaded last so nothing above clobbers them.
        "ldp d0, d1, [x1]",
        "ldp d2, d3, [x1, #16]",
        "ldp d4, d5, [x1, #32]",
        "ldp d6, d7, [x1, #48]",
        "blr x0",
        // The callee preserved x29; drop the spill area through it.
        "mov sp, x29",
        "ldp x29, x30, [sp], #16",
        cfi!(".cfi_def_cfa wsp, 0"),
        cfi!(".cfi_restore w30"),
        cfi!(".cfi_restore w29"),
        "ret",
        cfi!(".cfi_endproc"),
    )
}

/// Windows ARM64 keeps the previous shape: an `asm!` block inside an ordinary
/// Rust function. Unwinding there is SEH (`js_throw` raises with
/// `RaiseException`; the runtime walks no native frames — shadow frames), and
/// SEH reads `.pdata`/`.xdata` unwind codes that the COMPILER emits for the
/// prologue it generates — a frame-chained one on this ABI, so the dynamic
/// `sp` adjustment below is already invisible to it. A naked function would
/// have to hand-write those codes with nothing in this tree able to test
/// them, and would be treated as a leaf until it did — the callee's `blr`
/// has overwritten `x30`, so a throw through it could not find its handler.
#[cfg(all(target_arch = "aarch64", target_os = "windows"))]
#[inline(never)]
unsafe extern "C" fn call_all_f64_aarch64(
    func_ptr: usize,
    reg: *const f64,
    stacked: *const f64,
    stacked_count: usize,
    stack_bytes: usize,
) -> f64 {
    use core::arch::asm;

    let reg: [f64; 8] = unsafe { *reg.cast::<[f64; 8]>() };
    let ret: f64;
    asm!(
        // Stash the pre-adjust sp in a CALLEE-SAVED register (x20, declared as
        // a clobber so the compiler saves/restores it around this asm): it
        // survives the callee, unlike anything caller-saved.
        "mov x20, sp",
        "sub sp, sp, {stack_bytes}",
        "mov {i}, xzr",
        "cbz {cnt}, 3f",
        "2:",
        "ldr {tmp}, [{src}, {i}, lsl #3]",
        "str {tmp}, [sp, {i}, lsl #3]",
        "add {i}, {i}, #1",
        "cmp {i}, {cnt}",
        "b.lo 2b",
        "3:",
        "blr {func}",
        "mov sp, x20",
        func = in(reg) func_ptr,
        src = in(reg) stacked,
        cnt = in(reg) stacked_count,
        stack_bytes = in(reg) stack_bytes,
        i = out(reg) _,
        tmp = out(reg) _,
        out("x20") _,
        inout("d0") reg[0] => ret,
        inout("d1") reg[1] => _,
        inout("d2") reg[2] => _,
        inout("d3") reg[3] => _,
        inout("d4") reg[4] => _,
        inout("d5") reg[5] => _,
        inout("d6") reg[6] => _,
        inout("d7") reg[7] => _,
        // Caller-saved registers the callee may clobber (AAPCS64): x0–x17,
        // x30 (lr), and the caller-saved vector registers v16–v31.
        lateout("x0") _, lateout("x1") _, lateout("x2") _, lateout("x3") _,
        lateout("x4") _, lateout("x5") _, lateout("x6") _, lateout("x7") _,
        lateout("x8") _, lateout("x9") _, lateout("x10") _, lateout("x11") _,
        lateout("x12") _, lateout("x13") _, lateout("x14") _, lateout("x15") _,
        lateout("x16") _, lateout("x17") _, lateout("x30") _,
        lateout("v16") _, lateout("v17") _, lateout("v18") _, lateout("v19") _,
        lateout("v20") _, lateout("v21") _, lateout("v22") _, lateout("v23") _,
        lateout("v24") _, lateout("v25") _, lateout("v26") _, lateout("v27") _,
        lateout("v28") _, lateout("v29") _, lateout("v30") _, lateout("v31") _,
    );
    ret
}

/// SysV x86-64: `rdi` = callee, `rsi` = the eight register args, `rdx`/`rcx` =
/// the spilled args and their count, `r8` = the 16-byte-rounded spill area
/// size. Register args go in `xmm0`–`xmm7`; args 9+ are copied to
/// `[rsp + i*8]` in order. `al` carries the vector-register count a variadic
/// callee would want; Perry callees are non-variadic, but setting it is
/// harmless and matches the ABI requirement.
///
/// Alignment: `rsp ≡ 8 (mod 16)` at entry, `≡ 0` after the `push`, unchanged
/// by the 16-byte-multiple `sub`, so the `call` leaves the callee entry at
/// `≡ 8` per SysV.
#[cfg(all(target_arch = "x86_64", not(target_os = "windows")))]
#[unsafe(naked)]
unsafe extern "C" fn call_all_f64_x86_64(
    func_ptr: usize,
    reg: *const f64,
    stacked: *const f64,
    stacked_count: usize,
    stack_bytes: usize,
) -> f64 {
    core::arch::naked_asm!(
        // Frame pointer first, before rsp moves by a runtime amount.
        // Naked functions get no CFI region from rustc; this body is its own.
        cfi!(".cfi_startproc"),
        "push rbp",
        cfi!(".cfi_def_cfa_offset 16"),
        cfi!(".cfi_offset rbp, -16"),
        "mov rbp, rsp",
        cfi!(".cfi_def_cfa_register rbp"),
        // Reserve the (16-byte multiple) spill area and copy args 9+ into it.
        "sub rsp, r8",
        "xor eax, eax",
        "test rcx, rcx",
        "jz 3f",
        "2:",
        "mov r9, qword ptr [rdx + rax*8]",
        "mov qword ptr [rsp + rax*8], r9",
        "inc rax",
        "cmp rax, rcx",
        "jb 2b",
        "3:",
        // FP argument registers, loaded last so nothing above clobbers them.
        "movsd xmm0, qword ptr [rsi]",
        "movsd xmm1, qword ptr [rsi + 8]",
        "movsd xmm2, qword ptr [rsi + 16]",
        "movsd xmm3, qword ptr [rsi + 24]",
        "movsd xmm4, qword ptr [rsi + 32]",
        "movsd xmm5, qword ptr [rsi + 40]",
        "movsd xmm6, qword ptr [rsi + 48]",
        "movsd xmm7, qword ptr [rsi + 56]",
        "mov eax, 8",
        "call rdi",
        // The callee preserved rbp; drop the spill area through it.
        "mov rsp, rbp",
        "pop rbp",
        cfi!(".cfi_def_cfa rsp, 8"),
        cfi!(".cfi_restore rbp"),
        "ret",
        cfi!(".cfi_endproc"),
    )
}

/// Portable fallback for non-asm targets (incl. Windows x64, whose ABI differs
/// from the SysV asm above): fixed-arity dispatch up to 16 f64 args. No current
/// Perry host other than SysV aarch64/x86-64 exercises high-arity dynamic ctor
/// dispatch, so this bound is sufficient there. Arities > 16 FAIL CLOSED: a
/// fixed 16-arg `transmute` would mis-call the fn pointer with the wrong
/// signature (reading register/stack garbage for the missing params), the exact
/// silent-miscompile class that motivated #5437 — so we panic instead.
#[cfg(not(any(
    target_arch = "aarch64",
    all(target_arch = "x86_64", not(target_os = "windows"))
)))]
#[inline(never)]
unsafe fn call_all_f64_fallback(func_ptr: usize, args: &[f64]) -> f64 {
    #[inline(always)]
    fn a(args: &[f64], i: usize) -> f64 {
        args.get(i)
            .copied()
            .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED))
    }
    macro_rules! arm {
        ($($i:expr),*) => {{
            let f: extern "C" fn($(replace_expr!($i, f64)),*) -> f64 =
                std::mem::transmute(func_ptr);
            f($(a(args, $i)),*)
        }};
    }
    macro_rules! replace_expr {
        ($_t:expr, $sub:ty) => {
            $sub
        };
    }
    // args already includes `this` as element 0.
    match args.len() {
        0 => 0.0,
        1 => arm!(0),
        2 => arm!(0, 1),
        3 => arm!(0, 1, 2),
        4 => arm!(0, 1, 2, 3),
        5 => arm!(0, 1, 2, 3, 4),
        6 => arm!(0, 1, 2, 3, 4, 5),
        7 => arm!(0, 1, 2, 3, 4, 5, 6),
        8 => arm!(0, 1, 2, 3, 4, 5, 6, 7),
        9 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8),
        10 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9),
        11 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10),
        12 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11),
        13 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12),
        14 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13),
        15 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14),
        16 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15),
        // FAIL CLOSED: do NOT transmute a >16-arg call to a 16-arg signature —
        // the extra params would read register/stack garbage (#5437). This
        // target has no asm trampoline; high-arity dynamic dispatch is
        // unsupported here.
        n => panic!(
            "abi_trampoline: unsupported arity {n} on this target \
             (no asm trampoline; portable fallback caps at 16 f64 args)"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A 70-param all-f64 callee: returns arg0*1 + arg1*2 + ... weighted sum so
    // a misplaced/garbage arg is detectable, plus marker on the last few.
    #[cfg(any(
        target_arch = "aarch64",
        all(target_arch = "x86_64", not(target_os = "windows"))
    ))]
    extern "C" fn sum70(
        a0: f64,
        a1: f64,
        a2: f64,
        a3: f64,
        a4: f64,
        a5: f64,
        a6: f64,
        a7: f64,
        a8: f64,
        a9: f64,
        a10: f64,
        a11: f64,
        a12: f64,
        a13: f64,
        a14: f64,
        a15: f64,
        a16: f64,
        a17: f64,
        a18: f64,
        a19: f64,
        a20: f64,
        a21: f64,
        a22: f64,
        a23: f64,
        a24: f64,
        a25: f64,
        a26: f64,
        a27: f64,
        a28: f64,
        a29: f64,
        a30: f64,
        a31: f64,
        a32: f64,
        a33: f64,
        a34: f64,
        a35: f64,
        a36: f64,
        a37: f64,
        a38: f64,
        a39: f64,
        a40: f64,
        a41: f64,
        a42: f64,
        a43: f64,
        a44: f64,
        a45: f64,
        a46: f64,
        a47: f64,
        a48: f64,
        a49: f64,
        a50: f64,
        a51: f64,
        a52: f64,
        a53: f64,
        a54: f64,
        a55: f64,
        a56: f64,
        a57: f64,
        a58: f64,
        a59: f64,
        a60: f64,
        a61: f64,
        a62: f64,
        a63: f64,
        a64: f64,
        a65: f64,
        a66: f64,
        a67: f64,
        a68: f64,
        a69: f64,
    ) -> f64 {
        let xs = [
            a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15, a16, a17, a18,
            a19, a20, a21, a22, a23, a24, a25, a26, a27, a28, a29, a30, a31, a32, a33, a34, a35,
            a36, a37, a38, a39, a40, a41, a42, a43, a44, a45, a46, a47, a48, a49, a50, a51, a52,
            a53, a54, a55, a56, a57, a58, a59, a60, a61, a62, a63, a64, a65, a66, a67, a68, a69,
        ];
        let mut acc = 0.0;
        for (i, x) in xs.iter().enumerate() {
            acc += x * (i as f64 + 1.0);
        }
        acc
    }

    // High-arity (>16) dynamic dispatch only works on the asm targets; the
    // portable fallback fails closed (panics) above 16 args, so this test is
    // gated to the SysV asm targets.
    #[cfg(any(
        target_arch = "aarch64",
        all(target_arch = "x86_64", not(target_os = "windows"))
    ))]
    #[test]
    fn trampoline_passes_70_args_in_order() {
        // args = [this=100, then 69 values 1..=69]. call_all_f64 takes the full
        // arg list including `this` as element 0 → 70 total → sum70.
        let mut args = Vec::with_capacity(70);
        args.push(100.0); // a0 (this)
        for i in 1..70 {
            args.push(i as f64);
        }
        let got = unsafe { call_all_f64(sum70 as *const () as usize, &args) };
        // expected = sum(args[i]*(i+1))
        let expected: f64 = args
            .iter()
            .enumerate()
            .map(|(i, x)| x * (i as f64 + 1.0))
            .sum();
        assert_eq!(got, expected, "trampoline mis-ordered args");
    }

    extern "C" fn pick(
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        e: f64,
        f: f64,
        g: f64,
        h: f64,
        i: f64,
        j: f64,
    ) -> f64 {
        // beyond-register-window pick: returns the 9th and 10th (stack) args
        // combined so a stack-spill bug is caught.
        let _ = (a, b, c, d, e, f, g, h);
        i * 1000.0 + j
    }

    #[test]
    fn trampoline_stack_spill_args_9_and_10() {
        let args = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 42.0, 7.0];
        let got = unsafe { call_all_f64(pick as *const () as usize, &args) };
        assert_eq!(got, 42.0 * 1000.0 + 7.0);
    }

    // #9446: the unwinder must be able to step THROUGH the trampoline while the
    // callee runs. With stacked arguments the trampoline has lowered the stack
    // pointer by a runtime amount; a frame description that does not account
    // for that hands every unwinder — the GC's native-root walk, the exception
    // transport, gdb — a garbage return address for the trampoline's caller,
    // and the walk either stops there (silently dropping every frame above the
    // trampoline from the root set) or faults probing the garbage. This is a
    // differential test: the frames ABOVE the caller must look identical
    // whether the stack is walked from the caller itself or from inside a
    // callee reached through the trampoline with four stacked arguments.
    // `_Unwind_Backtrace` is what `gc/roots/stack_maps.rs` walks with, so it
    // is what this asks.
    #[cfg(all(
        any(target_os = "linux", target_vendor = "apple"),
        any(
            target_arch = "aarch64",
            all(target_arch = "x86_64", not(target_os = "windows"))
        )
    ))]
    mod unwind_through_the_trampoline {
        use super::call_all_f64;
        use crate::eh::UnwindContext;
        use std::cell::RefCell;
        use std::ffi::c_void;

        unsafe extern "C" {
            fn _Unwind_Backtrace(
                trace: unsafe extern "C" fn(*mut UnwindContext, *mut c_void) -> i32,
                argument: *mut c_void,
            ) -> i32;
            fn _Unwind_GetIP(context: *mut UnwindContext) -> usize;
        }

        /// `_URC_NO_REASON`, the only code that continues the walk.
        const URC_NO_REASON: i32 = 0;
        /// `_URC_END_OF_STACK`: stop, normally.
        const URC_END_OF_STACK: i32 = 5;
        /// Well above any test-harness depth; a walk that runs away must fail
        /// the assertion, not hang the suite.
        const MAX_FRAMES: usize = 256;

        unsafe extern "C" fn collect(context: *mut UnwindContext, argument: *mut c_void) -> i32 {
            let out = unsafe { &mut *argument.cast::<Vec<usize>>() };
            out.push(unsafe { _Unwind_GetIP(context) });
            if out.len() >= MAX_FRAMES {
                URC_END_OF_STACK
            } else {
                URC_NO_REASON
            }
        }

        /// Return addresses innermost-first, starting at this function's own
        /// frame. `inline(never)` so both walks below start one frame deep and
        /// the indexing that follows means the same thing on every host.
        #[inline(never)]
        fn return_addresses() -> Vec<usize> {
            let mut ips: Vec<usize> = Vec::new();
            unsafe {
                _Unwind_Backtrace(collect, (&mut ips as *mut Vec<usize>).cast::<c_void>());
            }
            std::hint::black_box(ips)
        }

        thread_local! {
            static SEEN_FROM_CALLEE: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
        }

        /// Twelve f64 params: eight in registers, four stacked on both asm
        /// ABIs. Walks the stack from inside, and returns two of the stacked
        /// args so a mis-passed spill is caught by the same test.
        extern "C" fn walking_callee(
            a0: f64,
            a1: f64,
            a2: f64,
            a3: f64,
            a4: f64,
            a5: f64,
            a6: f64,
            a7: f64,
            a8: f64,
            a9: f64,
            a10: f64,
            a11: f64,
        ) -> f64 {
            let _ = (a0, a1, a2, a3, a4, a5, a6, a7, a9, a10);
            SEEN_FROM_CALLEE.with(|seen| *seen.borrow_mut() = return_addresses());
            a8 * 1000.0 + a11
        }

        /// The caller frame the two walks have in common. Returns the walk
        /// taken from here and the walk taken from inside the callee.
        #[inline(never)]
        fn walk_from_caller_and_from_callee() -> (Vec<usize>, Vec<usize>) {
            let from_caller = return_addresses();
            let args: Vec<f64> = (0..12).map(f64::from).collect();
            let got = unsafe { call_all_f64(walking_callee as *const () as usize, &args) };
            assert_eq!(got, 8.0 * 1000.0 + 11.0, "stacked args mis-passed");
            let from_callee = SEEN_FROM_CALLEE.with(|seen| std::mem::take(&mut *seen.borrow_mut()));
            (from_caller, from_callee)
        }

        #[test]
        fn the_unwinder_steps_through_a_trampoline_with_stacked_args() {
            let (from_caller, from_callee) = walk_from_caller_and_from_callee();
            // `from_caller` is [return_addresses, walk_from_caller_and_from_callee,
            // harness…]; `from_callee` is [return_addresses, walking_callee,
            // trampoline, walk_from_caller_and_from_callee, harness…]. The two
            // sites inside the shared caller differ, everything above it must
            // not.
            assert!(
                from_caller.len() >= 3,
                "the control walk saw only {} frame(s); the harness above the \
                 caller is what the comparison is made of",
                from_caller.len()
            );
            let above_caller = &from_caller[2..];
            assert!(
                from_callee.len() >= above_caller.len() + 4,
                "walked from inside the trampoline's callee the unwinder saw \
                 {} frame(s), but the caller's own walk saw {} frames above \
                 the caller alone — the walk stopped at the trampoline, which \
                 is #9446: every frame above it is invisible to the GC's root \
                 scan while a dynamically dispatched callee runs.\n  from \
                 callee: {from_callee:#x?}\n  from caller: {from_caller:#x?}",
                from_callee.len(),
                above_caller.len()
            );
            let tail = &from_callee[from_callee.len() - above_caller.len()..];
            assert_eq!(
                tail, above_caller,
                "the frames above the caller must be reachable, and the same, \
                 through the trampoline.\n  from callee: {from_callee:#x?}\n  \
                 from caller: {from_caller:#x?}"
            );
        }
    }
}
