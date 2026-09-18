// #2875: TC39 explicit-resource-management globals — DisposableStack,
// AsyncDisposableStack, and SuppressedError must exist as real global
// constructors (typeof === "function") and the synchronous DisposableStack
// must run its registered disposers LIFO, expose a `.disposed` flag, and be
// idempotent on a second dispose(). SuppressedError must carry the
// `.name` / `.message` / `.error` / `.suppressed` shape and be an Error.

// 1) typeof of the three constructors.
console.log(typeof DisposableStack);
console.log(typeof AsyncDisposableStack);
console.log(typeof SuppressedError);

// 2) DisposableStack: defer returns undefined, use/defer/adopt all run LIFO.
const calls: string[] = [];
const stack = new DisposableStack();
console.log(typeof stack.defer(() => calls.push("defer")));
stack.use({ [Symbol.dispose]: () => calls.push("use") });
stack.adopt("resource", (v: string) => calls.push("adopt:" + v));
console.log(stack.disposed);
stack.dispose();
console.log(calls.join(","));
console.log(stack.disposed);

// 3) dispose() is idempotent — a second call does not re-run disposers.
stack.dispose();
console.log(calls.join(","));

// 4) SuppressedError shape.
const err = new SuppressedError(
  new Error("inner"),
  new Error("outer"),
  "both failed",
);
console.log(err.name);
console.log(err.message);
console.log((err.error as Error).message);
console.log((err.suppressed as Error).message);
console.log(err instanceof Error);
console.log(err instanceof SuppressedError);

// 5) The `instanceof Error` above must hold because the CHAIN says so, not
// because a registry happened to answer first: ECMA-262 gives
// `%SuppressedError%` the [[Prototype]] `%Error%` and
// `%SuppressedError.prototype%` the [[Prototype]] `%Error.prototype%`, exactly
// like the NativeErrors. Assert both links directly, and assert the inherited
// `Error.prototype.toString` behaviour that only a real chain can produce —
// without them `instanceof` regressed to `false` the moment it started
// answering from the recorded prototype walk.
console.log(Object.getPrototypeOf(SuppressedError) === Error);
console.log(Object.getPrototypeOf(SuppressedError.prototype) === Error.prototype);
console.log(Object.getPrototypeOf(TypeError.prototype) === Error.prototype);
console.log(Object.prototype.toString.call(err));
console.log(String(err));
console.log(JSON.stringify(new SuppressedError(new Error("a"), new Error("b")).message));
