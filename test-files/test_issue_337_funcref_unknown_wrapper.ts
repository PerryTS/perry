// Issue #337: codegen emitted js_closure_alloc(@__perry_wrap_perry_unknown_func)
// when an Expr::FuncRef's func_id wasn't in func_names — clang errored at
// IR validation. Fix: always emit the fallback wrapper that returns
// TAG_UNDEFINED, matching the fail-closed shape of the extern-class
// wrappers.
//
// The shape that triggers it: an `Iterable.ts`-style higher-order function
// reference where the callee can't be statically resolved. The minimal
// shape here is a function that explicitly returns another function and
// then references it indirectly through a path that loses the func_id
// during HIR lowering.

declare const externalFn: (x: number) => number;

// Higher-order use of an unresolvable identifier. With `declare const`,
// the variable has no init — codegen falls through to the FuncRef path
// when the receiver is a known-callable.
function safeMap(arr: number[], fn: (x: number) => number): number[] {
    const out: number[] = [];
    for (let i = 0; i < arr.length; i++) {
        out.push(fn(arr[i]));
    }
    return out;
}

// The compile target: even if `externalFn` resolves at runtime to
// undefined, the program shouldn't fail at link time. A runtime
// `safeMap([1,2,3], externalFn)` call would then either crash on the
// undefined dispatch (acceptable — fail-closed) or be replaced by the
// user.
const fns: Array<(x: number) => number> = [
    (x) => x + 1,
    (x) => x * 2,
];
const result = safeMap([1, 2, 3], fns[0]);
console.log(result.join(","));
