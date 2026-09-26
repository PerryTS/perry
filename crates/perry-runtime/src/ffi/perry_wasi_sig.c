/*
 * WASI only (#11379): report how many f64 arguments a closure body takes.
 *
 * Native C ABIs let the runtime call a closure body with more arguments than
 * it declares (the extras are ignored) or fewer (it reads garbage the dispatch
 * layer pads anyway). A wasm `call_indirect` whose type differs from its
 * target's traps, and a Perry closure body's real parameter count is not
 * always what the runtime registered for it: runtime thunks are declared with
 * whatever signature suits them and registered with their JS `.length`.
 *
 * `__builtin_wasm_test_function_pointer_signature` compiles to `ref.test`
 * (the GC proposal, hence `-mgc` on this file only), which asks the engine for
 * the function's actual type. The dispatch layer calls this once per body and
 * caches the answer, then always calls with exactly that many arguments.
 */

typedef struct ClosureHeader ClosureHeader;

typedef double (*perry_body_0)(const ClosureHeader *);
typedef double (*perry_body_1)(const ClosureHeader *, double);
typedef double (*perry_body_2)(const ClosureHeader *, double, double);
typedef double (*perry_body_3)(const ClosureHeader *, double, double, double);
typedef double (*perry_body_4)(const ClosureHeader *, double, double, double, double);
typedef double (*perry_body_5)(const ClosureHeader *, double, double, double, double, double);
typedef double (*perry_body_6)(const ClosureHeader *, double, double, double, double, double, double);
typedef double (*perry_body_7)(const ClosureHeader *, double, double, double, double, double, double, double);
typedef double (*perry_body_8)(const ClosureHeader *, double, double, double, double, double, double, double, double);
typedef double (*perry_body_9)(const ClosureHeader *, double, double, double, double, double, double, double, double, double);
typedef double (*perry_body_10)(const ClosureHeader *, double, double, double, double, double, double, double, double, double, double);
typedef double (*perry_body_11)(const ClosureHeader *, double, double, double, double, double, double, double, double, double, double, double);
typedef double (*perry_body_12)(const ClosureHeader *, double, double, double, double, double, double, double, double, double, double, double, double);
typedef double (*perry_body_13)(const ClosureHeader *, double, double, double, double, double, double, double, double, double, double, double, double, double);
typedef double (*perry_body_14)(const ClosureHeader *, double, double, double, double, double, double, double, double, double, double, double, double, double, double);
typedef double (*perry_body_15)(const ClosureHeader *, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double);
typedef double (*perry_body_16)(const ClosureHeader *, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double);
typedef double (*perry_body_17)(const ClosureHeader *, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double);
typedef double (*perry_body_18)(const ClosureHeader *, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double);
typedef double (*perry_body_19)(const ClosureHeader *, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double);
typedef double (*perry_body_20)(const ClosureHeader *, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double);
typedef double (*perry_body_21)(const ClosureHeader *, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double);
typedef double (*perry_body_22)(const ClosureHeader *, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double);
typedef double (*perry_body_23)(const ClosureHeader *, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double);
typedef double (*perry_body_24)(const ClosureHeader *, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double);
typedef double (*perry_body_25)(const ClosureHeader *, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double);
typedef double (*perry_body_26)(const ClosureHeader *, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double);
typedef double (*perry_body_27)(const ClosureHeader *, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double);
typedef double (*perry_body_28)(const ClosureHeader *, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double);
typedef double (*perry_body_29)(const ClosureHeader *, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double);
typedef double (*perry_body_30)(const ClosureHeader *, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double);
typedef double (*perry_body_31)(const ClosureHeader *, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double);
typedef double (*perry_body_32)(const ClosureHeader *, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double, double);

/* The body's f64 parameter count after the closure pointer, or -1 if its
 * type is none of `double (closure, double x k)` for k in 0..=32. */
int perry_wasi_closure_params(const void *fp) {
    if (__builtin_wasm_test_function_pointer_signature((perry_body_0)fp)) return 0;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_1)fp)) return 1;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_2)fp)) return 2;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_3)fp)) return 3;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_4)fp)) return 4;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_5)fp)) return 5;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_6)fp)) return 6;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_7)fp)) return 7;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_8)fp)) return 8;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_9)fp)) return 9;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_10)fp)) return 10;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_11)fp)) return 11;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_12)fp)) return 12;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_13)fp)) return 13;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_14)fp)) return 14;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_15)fp)) return 15;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_16)fp)) return 16;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_17)fp)) return 17;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_18)fp)) return 18;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_19)fp)) return 19;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_20)fp)) return 20;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_21)fp)) return 21;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_22)fp)) return 22;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_23)fp)) return 23;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_24)fp)) return 24;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_25)fp)) return 25;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_26)fp)) return 26;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_27)fp)) return 27;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_28)fp)) return 28;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_29)fp)) return 29;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_30)fp)) return 30;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_31)fp)) return 31;
    if (__builtin_wasm_test_function_pointer_signature((perry_body_32)fp)) return 32;
    return -1;
}
