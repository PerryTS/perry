// A spread argument forwarded into a native-instance method with a
// `NA_VARARGS` tail (`AsyncLocalStorage.prototype.run(store, cb, ...args)`)
// must be FLATTENED into individual arguments, not passed as a single array.
//
// The HIR instance-method arm produced a `NativeMethodCall` for the spread
// call using pre-lowered positional args (which collapse the spread source
// into one element); the `NA_VARARGS` packer then pushed that array as ONE
// argument. So `als.run(store, cb, ...[1, 2, 3])` handed `cb` a single
// `[1,2,3]` array instead of `1, 2, 3`. The static-method arms already
// guarded on `static_call_has_spread`; the instance arm was missing it, so
// spread instance calls now fall through to the flattening `CallSpread` path.
//
// Next.js wraps the whole app render in
// `workUnitAsyncStorage.run(store, renderFn, ...args)`, so a collapsed spread
// silently corrupted the forwarded arguments.
//
// Validated byte-for-byte against `node --experimental-strip-types`.

import { AsyncLocalStorage } from "async_hooks";

const als = new AsyncLocalStorage<{ s: number }>();
const arr = [1, 2, 3];

// spread of an array literal
console.log(als.run({ s: 1 }, (...a: number[]) => a.length, ...[1, 2, 3]));

// spread of a variable
console.log(als.run({ s: 1 }, (...a: number[]) => a.length, ...arr));

// mixed literal + spread + literal
console.log(als.run({ s: 1 }, (...a: number[]) => a.length, 0, ...arr, 9));

// forwarded args reach the callback in order and with correct values
console.log(als.run({ s: 1 }, (x: number, y: number, z: number) => x + y + z, ...arr));

// nested run(store, fn, ...restParam): the classic Next render shape
const outer = new AsyncLocalStorage<{ o: number }>();
const inner = new AsyncLocalStorage<{ i: number }>();
const nested = outer.run(
  { o: 1 },
  (fn: (a: number, b: number) => number, ...args: number[]) =>
    inner.run({ i: 2 }, fn, ...args),
  (a: number, b: number) => a + b,
  100,
  200,
);
console.log(nested);
