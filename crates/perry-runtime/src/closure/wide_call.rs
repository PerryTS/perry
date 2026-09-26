//! Closure-body calls wider than the exact per-arity dispatch arms (#10420).
//!
//! Dynamic dispatch reaches a closure body by transmuting its `func_ptr` to
//! `extern "C" fn(*const ClosureHeader, f64, …) -> f64` with the body's ABI
//! width: its declared params, plus the rest array and the `arguments` slot
//! when it has them. Rust has no variadic function types, so every width needs
//! its own transmute — `dispatch_with_arity` spells out 0..=32 and
//! `dispatch_rest_bundled` 0..=15 fixed params. Past those, both returned
//! `undefined` WITHOUT calling the body.
//!
//! Wider bodies go through a short ladder of PADDED widths instead: the slots
//! are copied into a buffer of the next ladder width, the tail is filled with
//! `undefined`, and the body is called through that wider signature. That is
//! sound for the same reason every `js_closure_callN` call with more arguments
//! than the body declares already is: on each ABI Perry targets (SysV x86-64,
//! AAPCS64 including Apple's variant, Win64) scalar arguments are assigned
//! left to right and the stack argument area belongs to the caller, so a body
//! declaring N doubles reads exactly the first N slots and ignores the rest.

use super::ClosureHeader;

/// Widest closure-body ABI dynamic dispatch can reach. A body wider than this
/// declares more than a thousand parameters; calling it through `apply`/`call`/
/// spread/a closure value throws a `RangeError` instead of silently skipping
/// the body.
pub(crate) const MAX_DYNAMIC_CALL_WIDTH: usize = 1024;

/// Call `func_ptr` as a closure body whose ABI takes `width` doubles, passing
/// `args[i]` for `i < min(args.len(), width)` and `undefined` for the rest.
///
/// Callers use this only past their exact arms, so `width` is at least 16.
/// `width` above [`MAX_DYNAMIC_CALL_WIDTH`] throws `RangeError`.
///
/// # Safety
/// `func_ptr` must be a validated, non-sentinel closure body whose ABI width
/// is at most `width`.
#[inline(never)]
pub(crate) unsafe fn dispatch_wide_abi(
    closure: *const ClosureHeader,
    func_ptr: *const u8,
    args: &[f64],
    width: usize,
) -> f64 {
    // WASI (#11378): the padding below relies on native C ABIs ignoring
    // surplus arguments; a wasm `call_indirect` must match its target's type
    // exactly, so a padded call would trap (and a 1025-parameter type is not
    // even a valid wasm component). Say so instead.
    #[cfg(target_os = "wasi")]
    {
        let _ = (closure, func_ptr, args);
        throw_too_wide_for_wasi(width)
    }
    #[cfg(not(target_os = "wasi"))]
    {
        let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
        let provided = args.len().min(width);

        // `padded!(slots, [0], 1, + + …)` doubles the index list once per `+`
        // (six doublings = 64 indices) and emits the transmuted call with one
        // `f64` parameter per index. The indices are constant expressions into a
        // fixed-size array, so the loads carry no bounds checks.
        macro_rules! padded {
        (@f64 $i:expr) => { f64 };
        ($slots:ident, [$($i:expr),+], $step:expr, + $($more:tt)*) => {
            padded!($slots, [$($i,)+ $($i + $step),+], $step * 2, $($more)*)
        };
        ($slots:ident, [$($i:expr),+], $step:expr,) => {{
            #[cfg(panic = "abort")]
            let f: extern "C" fn(*const ClosureHeader $(, padded!(@f64 $i))+) -> f64 =
                std::mem::transmute(func_ptr);
            #[cfg(not(panic = "abort"))]
            let f: extern "C-unwind" fn(*const ClosureHeader $(, padded!(@f64 $i))+) -> f64 =
                std::mem::transmute(func_ptr);
            f(closure $(, $slots[$i])+)
        }};
    }
        macro_rules! fill {
            ($len:literal) => {{
                let mut slots = [undef; $len];
                slots[..provided].copy_from_slice(&args[..provided]);
                slots
            }};
        }

        match width {
            0..=64 => {
                let slots = fill!(64);
                padded!(slots, [0usize], 1usize, + + + + + +)
            }
            65..=256 => {
                let slots = fill!(256);
                padded!(slots, [0usize], 1usize, + + + + + + + +)
            }
            257..=MAX_DYNAMIC_CALL_WIDTH => {
                let slots = fill!(1024);
                padded!(slots, [0usize], 1usize, + + + + + + + + + +)
            }
            _ => throw_too_wide(width),
        }
    }
}

#[cfg(target_os = "wasi")]
#[cold]
#[inline(never)]
fn throw_too_wide_for_wasi(width: usize) -> ! {
    let message = format!(
        "Calling a closure that takes {width} parameter slots dynamically is not \
         supported on WASI yet (#11378): wasm needs an exact-arity call"
    );
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_rangeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

#[cold]
#[inline(never)]
fn throw_too_wide(width: usize) -> ! {
    let message = format!(
        "Maximum call width exceeded: the callee takes {width} parameter slots \
         (at most {MAX_DYNAMIC_CALL_WIDTH} can be passed dynamically)"
    );
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_rangeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNDEF: u64 = crate::value::TAG_UNDEFINED;

    extern "C" fn body_40(
        _: *const ClosureHeader,
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
    ) -> f64 {
        let all = [
            a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15, a16, a17, a18,
            a19, a20, a21, a22, a23, a24, a25, a26, a27, a28, a29, a30, a31, a32, a33, a34, a35,
            a36, a37, a38, a39,
        ];
        // Encode "which slots are undefined" and the sum of the defined ones so
        // a dropped, shifted, or zero-filled slot changes the result.
        let mut sum = 0.0;
        let mut undefined_mask = 0u64;
        for (i, v) in all.iter().enumerate() {
            if v.to_bits() == UNDEF {
                undefined_mask |= 1 << i;
            } else {
                sum += *v * (i as f64 + 1.0);
            }
        }
        sum + (undefined_mask as f64) * 1.0e6
    }

    fn expected(provided: usize) -> f64 {
        let mut sum = 0.0;
        let mut undefined_mask = 0u64;
        for i in 0..40 {
            if i < provided {
                sum += (i as f64 + 100.0) * (i as f64 + 1.0);
            } else {
                undefined_mask |= 1 << i;
            }
        }
        sum + (undefined_mask as f64) * 1.0e6
    }

    #[test]
    fn padded_widths_deliver_every_slot_in_order() {
        let body = body_40 as *const u8;
        let args: Vec<f64> = (0..40).map(|i| i as f64 + 100.0).collect();
        // Exactly the declared width, through each ladder rung.
        for width in [40usize, 64, 65, 256, 257, 1024] {
            let result = unsafe { dispatch_wide_abi(std::ptr::null(), body, &args, width) };
            assert_eq!(result, expected(40), "width {width}");
        }
    }

    #[test]
    fn missing_slots_are_padded_with_undefined() {
        let body = body_40 as *const u8;
        let args: Vec<f64> = (0..23).map(|i| i as f64 + 100.0).collect();
        let result = unsafe { dispatch_wide_abi(std::ptr::null(), body, &args, 40) };
        assert_eq!(result, expected(23));
    }

    /// `dispatch_with_arity` past its 32 exact arms used to return `undefined`
    /// without calling the body.
    #[test]
    fn declared_arity_past_the_exact_arms_reaches_the_body() {
        let body = body_40 as *const u8;
        let args: Vec<f64> = (0..40).map(|i| i as f64 + 100.0).collect();
        let result =
            unsafe { crate::closure::dispatch_with_arity(std::ptr::null(), body, &args, 40) };
        assert_eq!(result, expected(40));
    }

    extern "C" fn rest_after_16(
        _: *const ClosureHeader,
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
        rest: f64,
    ) -> f64 {
        let fixed = [
            a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15,
        ];
        let rest = crate::value::js_nanbox_get_pointer(rest) as *const crate::array::ArrayHeader;
        let rest_len = crate::array::js_array_length(rest);
        let last = crate::array::js_array_get_f64(rest, rest_len - 1);
        fixed.iter().sum::<f64>() * 1_000.0 + f64::from(rest_len) * 100.0 + last
    }

    /// `dispatch_rest_bundled` with 16+ fixed params used to return `undefined`
    /// without calling the body.
    #[test]
    fn rest_bundling_past_fifteen_fixed_params_reaches_the_body() {
        let body = rest_after_16 as *const u8;
        let args: Vec<f64> = (1..=20).map(f64::from).collect();
        let result = unsafe {
            crate::closure::dispatch_rest_bundled(
                std::ptr::null(),
                body,
                &args,
                16,
                crate::closure::registry::RestDispatchKind::UserRest,
            )
        };
        // fixed = 1..=16 (sum 136), rest = [17, 18, 19, 20].
        assert_eq!(result, 136.0 * 1_000.0 + 4.0 * 100.0 + 20.0);
    }

    #[test]
    fn slots_past_the_width_are_not_forwarded() {
        let body = body_40 as *const u8;
        let args: Vec<f64> = (0..40).map(|i| i as f64 + 100.0).collect();
        // A width of 30 means the body only owns 30 slots; 30..40 read as undefined.
        let result = unsafe { dispatch_wide_abi(std::ptr::null(), body, &args, 30) };
        assert_eq!(result, expected(30));
    }
}
