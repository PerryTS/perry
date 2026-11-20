Removed a per-call heap allocation from `dispatch_bound_function`
(`crates/perry-runtime/src/closure/dispatch/bound.rs`), the runtime entry
every `js_closure_call<N>` routes a `Function.prototype.bind` result through.

The function unconditionally built a `Vec<f64>` (`Vec::with_capacity(args.len()
+ 4)`, a push loop over any bound args, then `extend_from_slice(args)`) before
calling the bound target, even for the overwhelmingly common `.bind(thisArg)`
shape with **no** partial-applied arguments — a plain method reference
(`arr.forEach(obj.method.bind(obj))`). `js_function_bind` already leaves
capture slot 2 (`bound_args_ptr`) null whenever no extra args were bound, so
that case now skips the allocate/copy/free cycle entirely and passes the
caller's own `args` slice straight through to `js_native_call_value`. The
actual partial-application shape (`.bind(obj, extra)`) is untouched other than
a tighter `Vec` capacity hint (`n + args.len()` instead of the old
`args.len() + 4`).

This targets the "method reference passed as callback" shape specifically:
unlike a plain closure held in a local, a `.bind()`-created function is
excluded from `direct.rs`'s per-loop dispatch hoisting (`BoundFunction` is
deliberately not resolved by `resolve_direct_func_ptr` —
`crates/perry-runtime/src/closure/dispatch/direct.rs:70`, inside the
`func_ptr.is_null() || func_ptr == BOUND_METHOD_FUNC_PTR || func_ptr ==
BOUND_FUNCTION_FUNC_PTR` early-return at lines 68-73), so `arr.forEach(fn)`
over a bound method re-runs this allocation on every element.

**Aliasing / rooting**: traced every one of `dispatch_bound_function`'s 9
call sites (exhaustive repo grep, not just perry-runtime). Every one hands it
either a `[f64; N]` literal array of by-value `f64` parameters living on the
Rust native stack (`closure/dispatch/calln.rs`'s per-arity entry points,
lines 38/83/151/181/215/255/299, and `dispatch_registered_call`'s 8 callers
at lines 382/415/449/496/549/606/665/727, each building `let args = [arg0,
arg1, ...]`) or a freshly Rust-`Vec`-allocated copy (`value_call.rs`'s
`full`, built by a `push` loop over `a(i)` before dispatch, for the >16-arg /
dynamic-call path). Neither shape is ever GC-managed memory — the collector
only owns memory it allocates itself (the `js_*_alloc` family into the
arena/nursery/old-gen) and has no knowledge of the Rust stack or
`Vec`/`Box` heap. So `args`'s *backing storage* can't be moved or freed by a
collection inside this call on any path, before or after this change, and
handing `js_native_call_value` the caller's slice directly instead of a
byte-copy of it is safe.

That is a narrower claim than "GC-safe" and worth stating precisely: a stack
array of NaN-boxed values is not a GC root, and neither was the old `Vec`
copy. If a collection *moves* an object referenced by one of the argument
*values* during `js_native_call_value` (e.g. inside `rebind_explicit_this`,
which can allocate), neither the old buffer nor the new one gets its bits
rewritten — copying bytes into a fresh `Vec` is not registering a root, so it
never protected against that. This change neither introduces nor fixes that
pre-existing exposure; it is identical before and after (and the conservative
native-stack scan that could theoretically cover it is diagnostic-only by
default, `Auto` → `SkipDisabled`).

Measured with the repo's differential probe technique (marginal cost isolated
from loop overhead, instructions retired plus wall/CPU time under a
measurement mutex, best-of-N): the `.bind(obj)`-with-no-extra-args shape drops
from ~2795 to ~2676 instructions per call (~4.3%), reproduced across two
independent runs (N=50000 and N=250000, 7 and 15 reps). Wall-clock time on the
measurement host was noisy under heavy unrelated contention (system load
averaged 60-117 on a 10-core box) and did not resolve a stable direction; the
more contention-robust process CPU-time metric (user+sys) showed no
regression (flat-to-favorable across both runs). The partially-applied-bind
shape (`.bind(obj, extra)`), the already-optimized loop-local-closure shape,
and a bare-loop control all measured ~0 delta, confirming the change is
isolated to its target shape.

Added `test-files/test_gap_callback_dispatch_shapes.ts`, byte-identical
against node 26.5.1, covering: direct arrow inline to a builtin array method,
a closure held in a local (once and in a loop), a callback threaded through a
second function frame, a callback parameter called directly in a loop, a
`.bind()` method reference with and without partial args, `this` binding
across arrow/ordinary/bound call shapes (including the receiverless-call
`this === undefined` case), `arguments`/extra/missing-argument/`.length`
handling, a callback that throws through one and two frames, recursion
through a plain and a bound callback reference, closures capturing a loop
variable (`let` and the classic `var` + IIFE idiom), and a bound method used
as a hot per-element `forEach` callback.
