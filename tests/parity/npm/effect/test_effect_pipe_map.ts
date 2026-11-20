// Effect.pipe(Effect.map(fn)) chain composition through the V8 boundary.
//
// Pre-fix (PR #992 ships `Effect.succeed(42)` but stops there):
//   - `Effect.map((x) => x + 1)` lowered via `StaticMethodCall { Effect, map }`
//     → `js_call_v8_member_method` (issue/PR #992 path).
//   - The inline arrow argument was NOT wrapped in `JsCreateCallback`
//     because the HIR `js_transform` pass only ran that rewrite for
//     `JsCallMethod` / `JsCallFunction` / callable JS values — the
//     `StaticMethodCall` arm just recursed on its args.
//   - At the V8 boundary the closure pointer crossed through
//     `fixup_native_for_v8` → POINTER_TAG → `native_object_to_v8`, which
//     misinterpreted the closure as a string/array/object proxy. V8 saw a
//     non-function and Effect's internal pipeline threw
//     "TypeError: f is not a function" inside `runSync`.
//
// Fix: wrap Closure args of `StaticMethodCall` on V8-imported classes in
// `JsCreateCallback`, and (defense in depth) make `native_object_to_v8`
// detect `GC_TYPE_CLOSURE` and wrap the closure as a v8::Function. Both
// paths surface the user's mapping function as a real JS callable so the
// chained `Effect.runSync(...)` returns the mapped value.

import { Effect } from 'effect';

const result = Effect.runSync(
    Effect.succeed(42).pipe(Effect.map((x: number) => x + 1)),
);
console.log('out=' + result);
