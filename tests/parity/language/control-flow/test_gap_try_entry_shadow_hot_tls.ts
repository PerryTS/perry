// Adversarial coverage for the `try`-entry perf change that moved `SHADOW`
// (`crates/perry-runtime/src/gc/roots/shadow_stack.rs`) from a raw
// `thread_local!` to `crate::perry_thread_local!` so `try_push_with_kind`'s
// `CatchSavepoint::capture()` stops paying a real `_tlv_get_addr` call on
// every non-throwing `try` entry. The change is a pure storage-mechanism
// swap (same type, same `.with()`/`.try_with()` call sites, same
// const-init/drop-free semantics), but the shadow stack IS the GC's precise
// root set for named locals and expression temporaries, and its savepoint is
// what a throw restores when unwinding skips frame-pop epilogues (#1830).
// This file exercises every shape that depends on that machinery staying
// exactly correct: a throw caught at the entry level, a throw several
// frames deep, a throw crossing a runtime helper callback, `finally` on both
// the normal and throwing paths, a nested `try` with an inner rethrow, a
// `catch` block that itself throws, deep `try` nesting, and — last — a
// non-throwing control so the fast (unchanged-behavior) path is checked too.
// Every caught value is read back AFTER GC-pressure allocation so a
// mis-rooted exception (swept, not just moved) would show up as wrong output
// rather than a crash.

function churn(n: number): number {
  const a: unknown[] = [];
  for (let i = 0; i < n; i++) {
    a.push({ i, s: "churn" + i, nested: { i } });
  }
  return a.length;
}

// 1. Throw caught at the same level.
function sameLevel(): string {
  try {
    throw new Error("same-level");
  } catch (e) {
    churn(300);
    return (e as Error).message;
  }
}
console.log("sameLevel", sameLevel());

// 2. Throw from a nested function several frames down.
function deep4(): number {
  throw new Error("deep4");
}
function deep3(): number {
  return deep4();
}
function deep2(): number {
  return deep3();
}
function deep1(): number {
  return deep2();
}
function nestedFrames(): string {
  try {
    deep1();
    return "unreached";
  } catch (e) {
    churn(300);
    return (e as Error).message;
  }
}
console.log("nestedFrames", nestedFrames());

// 3. Throw crossing a runtime helper (inside an Array.prototype.map
// callback — the throw unwinds through the runtime's own map trampoline).
function throughMap(): string {
  const src = [1, 2, 3, 4, 5];
  try {
    src.map((x) => {
      if (x === 3) throw new Error("through-map " + x);
      return x * 2;
    });
    return "unreached";
  } catch (e) {
    churn(300);
    return (e as Error).message;
  }
}
console.log("throughMap", throughMap());

// 4a. finally runs on the NORMAL (non-throwing) path.
let finallyNormalRuns = 0;
function finallyNormal(): number {
  try {
    return 42;
  } finally {
    finallyNormalRuns++;
  }
}
console.log("finallyNormal", finallyNormal(), "ran", finallyNormalRuns);

// 4b. finally runs on the THROWING path, then the exception still propagates.
let finallyThrowRuns = 0;
function finallyThrow(): number {
  try {
    throw new Error("finally-throw");
  } finally {
    finallyThrowRuns++;
  }
}
try {
  finallyThrow();
  console.log("finallyThrow (WRONG): did not throw");
} catch (e) {
  churn(300);
  console.log("finallyThrow caught", (e as Error).message, "ran", finallyThrowRuns);
}

// 5. Nested try with an inner rethrow — the outer catch must see the SAME
// (rethrown) exception, with its shadow-rooted fields intact.
function nestedRethrow(): string {
  try {
    try {
      const err: any = new Error("inner");
      err.tag = "rethrow-tag";
      throw err;
    } catch (inner) {
      churn(200);
      throw inner;
    }
  } catch (outer) {
    churn(200);
    const e = outer as any;
    return e.message + "/" + e.tag;
  }
}
console.log("nestedRethrow", nestedRethrow());

// 6. A try whose catch itself throws — the ORIGINAL exception is replaced by
// the catch's own throw, and outer code must see the new one.
function catchThrows(): string {
  try {
    try {
      throw new Error("first");
    } catch (e) {
      churn(200);
      throw new Error("from-catch:" + (e as Error).message);
    }
  } catch (e2) {
    churn(200);
    return (e2 as Error).message;
  }
}
console.log("catchThrows", catchThrows());

// 7. Deep try nesting — an exception thrown at the bottom must unwind
// through every level, running each finally exactly once, in order, and
// land in the outermost catch with the shadow stack balanced afterward.
const finallyOrder: number[] = [];
function deepNestTry(levels: number): string {
  if (levels === 0) {
    throw new Error("deep-nest-bottom");
  }
  try {
    return deepNestTry(levels - 1);
  } finally {
    finallyOrder.push(levels);
  }
}
try {
  deepNestTry(25);
  console.log("deepNestTry (WRONG): did not throw");
} catch (e) {
  churn(300);
  console.log(
    "deepNestTry caught",
    (e as Error).message,
    "finally count",
    finallyOrder.length,
    "finally order ok",
    finallyOrder.every((v, i) => v === i + 1)
  );
}

// A second, unrelated try AFTER the deep unwind: proves try_depth and the
// shadow stack were left exactly where they should be, not off by the
// number of levels just unwound.
function afterDeepUnwind(): string {
  try {
    throw new Error("after-deep-unwind");
  } catch (e) {
    return (e as Error).message;
  }
}
console.log("afterDeepUnwind", afterDeepUnwind());

// 8. Non-throwing control: the hot (unchanged-behavior) path. A tight loop
// of try/catch blocks that never throw, mixed with ones that do, so the
// fast entry/exit accounting can't drift relative to the slow throw path.
function controlLoop(): number {
  let total = 0;
  for (let i = 0; i < 200; i++) {
    try {
      total += i;
      if (i % 37 === 0) {
        try {
          if (i % 74 === 0) throw new Error("control-inner " + i);
          total += 1;
        } catch (e) {
          churn(20);
          total -= 1;
        }
      }
    } catch (e) {
      total = -1; // never reached
    }
  }
  return total;
}
console.log("controlLoop", controlLoop());

// Final sanity: try_depth is back at zero — a fresh top-level try still
// catches correctly after everything above.
try {
  throw new Error("final-sanity");
} catch (e) {
  console.log("finalSanity", (e as Error).message);
}
console.log("done");
