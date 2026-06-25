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
//! directly with inline assembly for the two hosted architectures (aarch64 +
//! x86-64); other targets fall back to a fixed-arity dispatch good to 16 args
//! (no Perry target other than the two asm ones exercises high-arity dynamic
//! ctor dispatch today).

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
        call_all_f64_aarch64(func_ptr, args)
    }
    // NOTE: gated to NON-Windows x86-64. The asm below is the SysV ABI (FP args
    // in xmm0..xmm7, no shadow space). The Windows x64 ABI passes FP args in
    // xmm0..xmm3 and requires a 32-byte shadow space, so the SysV asm would
    // mis-pass 5+ args. Win64 falls through to the portable fallback instead.
    #[cfg(all(target_arch = "x86_64", not(target_os = "windows")))]
    {
        call_all_f64_x86_64(func_ptr, args)
    }
    #[cfg(not(any(
        target_arch = "aarch64",
        all(target_arch = "x86_64", not(target_os = "windows"))
    )))]
    {
        call_all_f64_fallback(func_ptr, args)
    }
}

/// AAPCS64: the first 8 f64 args go in v0–v7; args 9+ are spilled to the stack
/// in order, each occupying 8 bytes, with the stack 16-byte aligned at the call.
#[cfg(target_arch = "aarch64")]
#[inline(never)]
unsafe fn call_all_f64_aarch64(func_ptr: usize, args: &[f64]) -> f64 {
    use core::arch::asm;

    let n = args.len();
    // Register args (up to 8); pad missing with 0.0 (callee won't read them).
    let mut reg = [0.0f64; 8];
    for (i, slot) in reg.iter_mut().enumerate() {
        if i < n {
            *slot = args[i];
        }
    }

    // Stack-spilled args: args[8..]. Bytes = stacked_count * 8, rounded up to a
    // 16-byte multiple so `sp` stays 16-aligned across the `blr`.
    let stacked = if n > 8 { &args[8..] } else { &[][..] };
    let stacked_count = stacked.len();
    let raw_bytes = stacked_count * 8;
    let stack_bytes = (raw_bytes + 15) & !15;

    let ret: f64;
    asm!(
        // Stash the current sp in a CALLEE-SAVED register (x20, declared as a
        // clobber below so the compiler saves/restores it around this asm). The
        // callee preserves callee-saved registers, so x20 survives the `blr` —
        // restoring sp from a caller-saved copy or by re-adding a clobbered
        // `stack_bytes` would corrupt the stack.
        "mov x20, sp",
        // Reserve aligned stack space for the spilled args.
        "sub sp, sp, {stack_bytes}",
        // Copy spilled args from the source pointer into [sp + i*8].
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
        // Restore sp from the callee-saved copy.
        "mov sp, x20",
        func = in(reg) func_ptr,
        src = in(reg) stacked.as_ptr(),
        cnt = in(reg) stacked_count,
        stack_bytes = in(reg) stack_bytes,
        i = out(reg) _,
        tmp = out(reg) _,
        out("x20") _,
        // FP argument registers v0–v7.
        inout("d0") reg[0] => ret,
        inout("d1") reg[1] => _,
        inout("d2") reg[2] => _,
        inout("d3") reg[3] => _,
        inout("d4") reg[4] => _,
        inout("d5") reg[5] => _,
        inout("d6") reg[6] => _,
        inout("d7") reg[7] => _,
        // Caller-saved registers the callee may clobber (AAPCS64). x0–x17 and
        // x30(lr) are call-clobbered GPRs; v8–v15 lower 64 bits are
        // callee-saved (preserved), v16–v31 are caller-saved.
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

/// SysV x86-64: the first 8 f64 args go in xmm0–xmm7; args 9+ are spilled to the
/// stack (each 8 bytes), with the stack 16-byte aligned at the `call`. `al` must
/// hold the number of vector registers used for a (possibly) variadic callee;
/// Perry callees are non-variadic, but setting `al` is harmless and matches the
/// ABI requirement for safety.
#[cfg(all(target_arch = "x86_64", not(target_os = "windows")))]
#[inline(never)]
unsafe fn call_all_f64_x86_64(func_ptr: usize, args: &[f64]) -> f64 {
    use core::arch::asm;

    let n = args.len();
    let mut reg = [0.0f64; 8];
    for (i, slot) in reg.iter_mut().enumerate() {
        if i < n {
            *slot = args[i];
        }
    }

    let stacked = if n > 8 { &args[8..] } else { &[][..] };
    let stacked_count = stacked.len();
    // Stack must be 16-aligned at the call instruction. The `call` pushes an
    // 8-byte return address, so before the `call` we need `sp % 16 == 0`. We
    // reserve a 16-byte multiple for the spilled args; if `stacked_count` is
    // odd, the natural 8-byte total would misalign, so round up.
    let raw_bytes = stacked_count * 8;
    let stack_bytes = (raw_bytes + 15) & !15;

    let ret: f64;
    asm!(
        // Stash the pre-adjust rsp in r12 (CALLEE-SAVED, declared as a clobber
        // below so the compiler saves/restores it). The callee preserves r12, so
        // the sp restore survives the callee clobbering every caller-saved
        // register (including any holding `stack_bytes`). We use r12 rather than
        // rbx because LLVM reserves rbx internally and rejects it as an explicit
        // inline-asm operand; r12 is an equivalent callee-saved scratch.
        "mov r12, rsp",
        // Reserve space for spilled args, then force rsp 16-aligned so that the
        // `call` (which pushes the 8-byte return address) leaves the callee
        // entry with rsp ≡ 8 (mod 16), per SysV. `stack_bytes` is a 16-multiple,
        // so aligning rsp down by clearing the low 4 bits keeps room for all
        // spilled slots (they are written relative to the post-align rsp).
        "sub rsp, {stack_bytes}",
        "and rsp, -16",
        "xor {i:e}, {i:e}",
        "test {cnt}, {cnt}",
        "jz 3f",
        "2:",
        "mov {tmp}, qword ptr [{src} + {i}*8]",
        "mov qword ptr [rsp + {i}*8], {tmp}",
        "inc {i}",
        "cmp {i}, {cnt}",
        "jb 2b",
        "3:",
        "call {func}",
        "mov rsp, r12",
        func = in(reg) func_ptr,
        src = in(reg) stacked.as_ptr(),
        cnt = in(reg) stacked_count,
        stack_bytes = in(reg) stack_bytes,
        i = out(reg) _,
        tmp = out(reg) _,
        out("r12") _,
        inout("xmm0") reg[0] => ret,
        inout("xmm1") reg[1] => _,
        inout("xmm2") reg[2] => _,
        inout("xmm3") reg[3] => _,
        inout("xmm4") reg[4] => _,
        inout("xmm5") reg[5] => _,
        inout("xmm6") reg[6] => _,
        inout("xmm7") reg[7] => _,
        // Caller-saved GPRs the callee may clobber (SysV). `al` (in rax) is set
        // to the FP-register count for variadic safety. xmm8–xmm15 are
        // caller-saved on SysV too.
        inout("rax") 8u64 => _, lateout("rcx") _, lateout("rdx") _, lateout("rsi") _,
        lateout("rdi") _, lateout("r8") _, lateout("r9") _, lateout("r10") _,
        lateout("r11") _,
        lateout("xmm8") _, lateout("xmm9") _, lateout("xmm10") _, lateout("xmm11") _,
        lateout("xmm12") _, lateout("xmm13") _, lateout("xmm14") _, lateout("xmm15") _,
    );
    ret
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
            let f: extern "C" fn($(replace_expr!($i f64)),*) -> f64 =
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
        let got = unsafe { call_all_f64(sum70 as usize, &args) };
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
        let got = unsafe { call_all_f64(pick as usize, &args) };
        assert_eq!(got, 42.0 * 1000.0 + 7.0);
    }
}
