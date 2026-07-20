//! Target-dependent setjmp ABI selection for try/catch lowering.
//!
//! Perry's try/catch (and the async rejection boundary) is setjmp/longjmp
//! based, and the setjmp *variant* differs per target:
//!
//!   - Windows MSVC: `_setjmp(jmp_buf, frame_pointer)` — MSVCRT exports only
//!     `_setjmp`/`_setjmpex` (there is no plain `setjmp` symbol), and the x64
//!     intrinsic takes the frame pointer as a second argument (we pass null
//!     to opt out of SEH unwinding through the frame).
//!   - Apple: the fast `_setjmp(jmp_buf)` (LLVM-IR name `_setjmp` → Mach-O
//!     symbol `__setjmp`), which skips the sigprocmask/sigaltstack syscalls
//!     the default `setjmp(3)` performs on Darwin (~43% of CPU on
//!     promise_all_chains.ts before the swap). Perry never longjmps out of a
//!     signal handler, so the fast variant is functionally equivalent.
//!   - Everything else (Linux glibc/musl, Android, OHOS): plain
//!     `setjmp(jmp_buf)` — glibc's `setjmp(3)` already skips the signal
//!     mask, so no swap is needed.
//!
//! The variant MUST be derived from the *compile target's* LLVM triple, not
//! from host `cfg!`. The host `cfg!` version of this decision meant
//! cross-compiling `--target windows` from Linux emitted `@setjmp` (LNK2019:
//! MSVCRT has no `setjmp` export), from macOS emitted the 1-arg `_setjmp`
//! (the MSVC x64 second argument — RDX — was garbage, so longjmp corrupted),
//! and a Windows host emitted the 2-arg Windows form into linux/macos
//! objects. Host-native compiles were always correct because
//! `default_target_triple()` (the `CompileOptions.target == None` fallback)
//! is the host triple — deriving from the effective triple preserves that
//! behavior bit-for-bit. Same principle as `crate::target_layout`.
//!
//! Every consumer — the try/catch call site, the async-boundary call site,
//! and the extern prototype in `runtime_decls` — must go through this one
//! type so the call and its declaration can never disagree on the callee
//! name or arity (that would be an LLVM verifier error, or silent stack
//! corruption).

use crate::types::{LlvmType, PTR};

/// Which setjmp flavor the emitted IR calls (and declares).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SetjmpAbi {
    /// `_setjmp(ptr jmp_buf, ptr frame_ptr)` — Windows MSVC x64.
    WindowsMsvc,
    /// `_setjmp(ptr jmp_buf)` — Apple's fast (no-sigprocmask) variant.
    AppleFast,
    /// `setjmp(ptr jmp_buf)` — default C ABI (Linux glibc/musl, Android, …).
    CSetjmp,
}

/// Select the setjmp ABI for an LLVM target triple. `triple` is the
/// *effective* triple for the compile — `CompileOptions.target` when
/// `--target` was given, else `default_target_triple()` (the host) — so a
/// host-native compile keeps today's behavior and a cross-compile follows
/// the target.
pub(crate) fn setjmp_abi_for_triple(triple: &str) -> SetjmpAbi {
    if triple.contains("-windows-") {
        SetjmpAbi::WindowsMsvc
    } else if triple.contains("-apple-") {
        SetjmpAbi::AppleFast
    } else {
        SetjmpAbi::CSetjmp
    }
}

impl SetjmpAbi {
    /// LLVM-IR callee name (also what `runtime_decls` declares).
    pub(crate) fn callee(self) -> &'static str {
        match self {
            SetjmpAbi::WindowsMsvc | SetjmpAbi::AppleFast => "_setjmp",
            SetjmpAbi::CSetjmp => "setjmp",
        }
    }

    /// Parameter types for the extern declaration. Must stay in lock-step
    /// with [`Self::call_instruction`] — the divergence test below enforces
    /// the arity.
    pub(crate) fn param_types(self) -> &'static [LlvmType] {
        match self {
            SetjmpAbi::WindowsMsvc => &[PTR, PTR],
            SetjmpAbi::AppleFast | SetjmpAbi::CSetjmp => &[PTR],
        }
    }

    /// The full call-site instruction. `#0` is the shared `returns_twice`
    /// attribute group emitted by `LlModule` whenever a setjmp variant is
    /// declared — it must be on the call site too, or LLVM -O2 promotes
    /// allocas across the setjmp and the longjmp path reads stale values.
    pub(crate) fn call_instruction(self, result_reg: &str, jmpbuf: &str) -> String {
        match self {
            SetjmpAbi::WindowsMsvc => format!(
                "{} = call i32 @_setjmp(ptr {}, ptr null) #0",
                result_reg, jmpbuf
            ),
            SetjmpAbi::AppleFast => {
                format!("{} = call i32 @_setjmp(ptr {}) #0", result_reg, jmpbuf)
            }
            SetjmpAbi::CSetjmp => {
                format!("{} = call i32 @setjmp(ptr {}) #0", result_reg, jmpbuf)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_target_uses_two_arg_underscore_setjmp() {
        let abi = setjmp_abi_for_triple("x86_64-pc-windows-msvc");
        assert_eq!(abi, SetjmpAbi::WindowsMsvc);
        assert_eq!(abi.callee(), "_setjmp");
        assert_eq!(abi.param_types(), &[PTR, PTR]);
        assert_eq!(
            abi.call_instruction("%r7", "%r6"),
            "%r7 = call i32 @_setjmp(ptr %r6, ptr null) #0"
        );
    }

    #[test]
    fn apple_targets_use_fast_one_arg_underscore_setjmp() {
        for triple in [
            "arm64-apple-macosx15.0.0",
            "x86_64-apple-macosx15.0.0",
            "aarch64-apple-darwin",
            "aarch64-apple-ios",
            "arm64-apple-ios17.0-simulator",
            "aarch64-apple-tvos",
            "aarch64-apple-watchos",
            "arm64_32-apple-watchos",
            "arm64-apple-xros1.0",
        ] {
            let abi = setjmp_abi_for_triple(triple);
            assert_eq!(abi, SetjmpAbi::AppleFast, "triple: {}", triple);
            assert_eq!(abi.callee(), "_setjmp");
            assert_eq!(abi.param_types(), &[PTR]);
            assert_eq!(
                abi.call_instruction("%r7", "%r6"),
                "%r7 = call i32 @_setjmp(ptr %r6) #0"
            );
        }
    }

    #[test]
    fn linux_family_targets_use_plain_setjmp() {
        for triple in [
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
            "x86_64-unknown-linux-musl",
            "aarch64-unknown-linux-musl",
            "aarch64-unknown-linux-android",
            "x86_64-unknown-linux-android",
            "aarch64-unknown-linux-ohos",
        ] {
            let abi = setjmp_abi_for_triple(triple);
            assert_eq!(abi, SetjmpAbi::CSetjmp, "triple: {}", triple);
            assert_eq!(abi.callee(), "setjmp");
            assert_eq!(abi.param_types(), &[PTR]);
            assert_eq!(
                abi.call_instruction("%r7", "%r6"),
                "%r7 = call i32 @setjmp(ptr %r6) #0"
            );
        }
    }

    /// The declaration arity and the call-site arity must never diverge —
    /// that's an IR verifier error (or, worse, a silently-garbage RDX on
    /// MSVC x64). Count the `ptr ` argument slots in the emitted call and
    /// compare against the declared parameter list, per variant.
    #[test]
    fn call_arity_matches_declared_arity_for_every_variant() {
        for abi in [
            SetjmpAbi::WindowsMsvc,
            SetjmpAbi::AppleFast,
            SetjmpAbi::CSetjmp,
        ] {
            let call = abi.call_instruction("%r1", "%r0");
            let arg_count = call.matches("ptr ").count();
            assert_eq!(
                arg_count,
                abi.param_types().len(),
                "call/declaration arity diverged for {:?}: {}",
                abi,
                call
            );
            assert!(
                call.contains(&format!("@{}(", abi.callee())),
                "call names a different callee than the declaration for {:?}: {}",
                abi,
                call
            );
        }
    }
}
