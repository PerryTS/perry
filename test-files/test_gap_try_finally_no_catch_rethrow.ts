// Issue #37 / #321: `try { ... } finally { ... }` with NO catch clause must
// run the finally and then RE-PROPAGATE the original exception — it must not
// be swallowed. Previously Perry's setjmp landing pad for a catch-less
// try/finally ran `js_try_end()`, fell through to the finally/merge, and
// returned `undefined`, silently eating the throw. This surfaced in the
// effect framework's `internalCall` "forced" path
// (`try { return body() } finally {}`) as a dropped return value.

// 1. Empty finally, body throws — exception must propagate.
function inner1(): number {
  throw new Error("boom");
}
function wrap1(): number {
  try {
    return inner1();
  } finally {
    // empty
  }
}
try {
  const r = wrap1();
  console.log("wrap1 returned (WRONG):", r);
} catch (e) {
  console.log("wrap1 caught:", (e as Error).message);
}

// 2. Non-empty finally, body throws — finally runs, then exception propagates.
let cleanups = 0;
function inner2(): string {
  throw new Error("kaboom");
}
function wrap2(): string {
  try {
    return inner2();
  } finally {
    cleanups++;
  }
}
try {
  wrap2();
  console.log("wrap2 returned (WRONG)");
} catch (e) {
  console.log("wrap2 caught:", (e as Error).message, "cleanups:", cleanups);
}

// 3. Normal completion (no throw) still returns the value and runs finally.
let ran = 0;
function wrap3(): number {
  try {
    return 99;
  } finally {
    ran++;
  }
}
console.log("wrap3:", wrap3(), "ran:", ran);

// 4. Indirect call through a function-valued variable (the effect shape):
//    a generic arrow holding `try { return body() } finally {}` that the
//    caller invokes; when body throws the exception must propagate.
const forced = <A>(body: () => A): A => {
  try {
    return body();
  } finally {
  }
};
try {
  forced(() => {
    throw new Error("indirect");
  });
  console.log("forced returned (WRONG)");
} catch (e) {
  console.log("forced caught:", (e as Error).message);
}

// 5. finally with its own `return` overrides the pending exception (spec).
function wrap5(): string {
  try {
    throw new Error("overridden");
  } finally {
    return "from-finally"; // eslint-disable-line no-unsafe-finally
  }
}
console.log("wrap5:", wrap5());

// 6. Nested catch-less try/finally inside an outer try/catch — propagates
//    out through both finallies to the outer catch.
let order: string[] = [];
function wrap6(): number {
  try {
    try {
      throw new Error("nested");
    } finally {
      order.push("inner-finally");
    }
  } finally {
    order.push("outer-finally");
  }
}
try {
  wrap6();
  console.log("wrap6 returned (WRONG)");
} catch (e) {
  console.log("wrap6 caught:", (e as Error).message, "order:", order.join(","));
}
