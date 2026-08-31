/* setjmp trampoline for the Rust-side exception transport (issue #9305).
 *
 * WHY THIS FILE EXISTS — read before touching any trap site:
 *
 * Rust cannot mark an extern function `returns_twice` (the unstable
 * `#[ffi_returns_twice]` attribute was removed from rustc), so a Rust
 * function that calls `setjmp` directly is compiled under LLVM's
 * one-return assumption. LLVM is then free to color stack slots across
 * the call: a spill slot that is live only into the longjmp-return
 * branch appears dead on the normal path, so an unrelated temporary can
 * be assigned the same slot. That is exactly what crashed
 * `run_microtasks` (#9305): the compiler's own cached TLS base pointer
 * (`mov %fs:0x0,%rax`) was spilled before the `setjmp`, the microtask
 * record-copy loop reused the slot on the normal path, and the longjmp
 * return path reloaded 0 -> NULL-based TLS access -> SIGSEGV. The
 * corrupted value was a compiler-generated temporary with no source
 * name, so no Rust-side discipline (re-reading state from TLS after the
 * jump, avoiding locals) can remove the hazard — it is a property of
 * the caller's code generation.
 *
 * The durable fix: NO RUST FRAME IS EVER A longjmp TARGET. This C
 * function is the only frame `setjmp` returns into twice, and it is
 * compiled by a C compiler that knows setjmp's contract (clang/gcc
 * recognize the name and apply `returns_twice`; MSVC lowers setjmp
 * intrinsically). Its only state live across the call — `env`, `body`,
 * `ctx` — is unmodified between `setjmp` and `longjmp`, which C
 * guarantees to be preserved. Rust callers observe `perry_sjlj_try` as
 * an ordinary single-return call, so every LLVM assumption about their
 * frames holds.
 *
 * `env` is a jmp_buf slab from `exception.rs::js_try_push` (`JmpBuf`:
 * 256 bytes, 16-byte aligned; see `ffi/setjmp.rs::JMP_BUF_MIN_BYTES`
 * for the per-platform layout notes — on glibc the saved-signal-mask
 * tail of `jmp_buf` is never written because glibc's `setjmp` does not
 * save the mask). `js_throw` longjmps to it with value 1 while this
 * frame is still live.
 *
 * Platform pairing matches the runtime's existing externs
 * (`ffi/setjmp.rs`): Apple targets use the fast `_setjmp(3)` (no
 * sigprocmask/sigaltstack round trip — measured at ~43% of microtask
 * pump CPU when the signal-saving variant was used); everywhere else
 * the plain `setjmp`. `js_throw`'s `longjmp` extern is unchanged, and
 * on Windows it still zeroes `_JUMP_BUFFER.Frame` before jumping to
 * force the non-unwinding POSIX-style longjmp (#7356).
 */

#include <setjmp.h>

typedef void (*perry_sjlj_body)(void *ctx);

#if defined(__APPLE__)
/* Redeclaring with an explicit attribute is belt-and-braces: clang
 * already treats `_setjmp` as returns_twice by name. */
extern int _setjmp(jmp_buf) __attribute__((returns_twice));
#define PERRY_SETJMP(env) _setjmp(env)
#else
#define PERRY_SETJMP(env) setjmp(env)
#endif

int perry_sjlj_try(void *env, perry_sjlj_body body, void *ctx) {
    int rc = PERRY_SETJMP(*(jmp_buf *)env);
    if (rc == 0)
        body(ctx);
    return rc;
}
