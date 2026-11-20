// Issue #12632: in an async function, a rejected `await` inside a `try` must
// resume as a throw AT that await — routed through the enclosing catch/finally
// exactly like a synchronous throw there. Before the fix, the step driver's
// rejection arm only knew `catch` routes, so a `finally` around the await was
// skipped and the rejection went straight to the caller.

const rej = (m: string): Promise<never> => Promise.reject(new Error(m));
const tick = (): Promise<void> => Promise.resolve();

// 1. The issue repro: try/finally, no catch.
async function f1(): Promise<void> {
  try {
    await rej("r1");
    console.log("1 not reached");
  } finally {
    console.log("1 finally");
  }
}

// 2. catch + finally: catch handles, finally runs, function resolves.
async function f2(): Promise<string> {
  try {
    await rej("r2");
    return "2 not reached";
  } catch (e) {
    console.log("2 catch", (e as Error).message);
  } finally {
    console.log("2 finally");
  }
  return "2 done";
}

// 3. Nested try/finally: both finallys run, innermost first.
async function f3(): Promise<void> {
  try {
    try {
      await tick();
      await rej("r3");
    } finally {
      console.log("3 inner finally");
    }
  } finally {
    console.log("3 outer finally");
  }
}

// 4. Inner try/finally inside an outer try/catch: inner finally runs before
//    the outer catch.
async function f4(): Promise<string> {
  try {
    try {
      await rej("r4");
    } finally {
      console.log("4 inner finally");
    }
  } catch (e) {
    console.log("4 outer catch", (e as Error).message);
  }
  return "4 done";
}

// 5. A finally that itself awaits.
async function f5(): Promise<void> {
  try {
    await rej("r5");
  } finally {
    console.log("5 finally start");
    await tick();
    console.log("5 finally after await");
  }
}

// 6. Rethrow from the catch: the finally still runs.
async function f6(): Promise<void> {
  try {
    await rej("r6");
  } catch (e) {
    console.log("6 catch");
    throw new Error("re-" + (e as Error).message);
  } finally {
    console.log("6 finally");
  }
}

// 7. return inside the try after an await that resolves: finally runs, value kept.
async function f7(): Promise<number> {
  try {
    await tick();
    return 7;
  } finally {
    console.log("7 finally");
  }
}

// 8. A finally that returns overrides the rejection.
async function f8(): Promise<string> {
  try {
    await rej("r8");
  } finally {
    // eslint-disable-next-line no-unsafe-finally
    return "8 overridden";
  }
}

// 9. Synchronous throw after an await inside try/finally.
async function f9(): Promise<void> {
  try {
    await tick();
    throw new Error("t9");
  } finally {
    console.log("9 finally");
  }
}

// 10. Rejection in a loop body with try/finally: finally runs per iteration.
async function f10(): Promise<void> {
  for (let i = 0; i < 2; i++) {
    try {
      if (i === 1) await rej("r10");
      else await tick();
      console.log("10 body", i);
    } finally {
      console.log("10 finally", i);
    }
  }
}

// 11. The finally awaits, rejection then caught by an enclosing catch.
async function f11(): Promise<string> {
  try {
    try {
      await rej("r11");
    } finally {
      await tick();
      console.log("11 inner finally");
    }
  } catch (e) {
    console.log("11 outer catch", (e as Error).message);
  }
  return "11 done";
}

// 12. Async arrow (closure) form.
const f12 = async (): Promise<void> => {
  try {
    await rej("r12");
  } finally {
    console.log("12 finally");
  }
};

// 13. Async class method form.
class C13 {
  async m(): Promise<void> {
    try {
      await rej("r13");
    } finally {
      console.log("13 finally");
    }
  }
}

// 14. An await inside the catch rejects: the finally of that try runs before
//     the enclosing catch sees the second error.
async function f14(): Promise<string> {
  try {
    try {
      await rej("r14a");
    } catch (e) {
      console.log("14 inner catch", (e as Error).message);
      await rej("r14b");
    } finally {
      console.log("14 inner finally");
    }
  } catch (e) {
    console.log("14 outer catch", (e as Error).message);
  }
  return "14 done";
}

// 15. A finally that throws replaces the rejection.
async function f15(): Promise<void> {
  try {
    await rej("r15");
  } finally {
    // eslint-disable-next-line no-unsafe-finally
    throw new Error("t15");
  }
}

// 16. Per-iteration nested try/finally inside try/catch with `continue`.
async function f16(): Promise<string> {
  for (let i = 0; i < 2; i++) {
    try {
      try {
        await rej("r16-" + i);
      } finally {
        console.log("16 inner finally", i);
      }
    } catch (e) {
      console.log("16 catch", (e as Error).message);
      continue;
    }
  }
  return "16 done";
}

async function report(label: string, p: Promise<unknown>): Promise<void> {
  try {
    const v = await p;
    console.log(label, "resolved", v);
  } catch (e) {
    console.log(label, "rejected", (e as Error).message);
  }
}

async function main(): Promise<void> {
  await report("1:", f1());
  await report("2:", f2());
  await report("3:", f3());
  await report("4:", f4());
  await report("5:", f5());
  await report("6:", f6());
  await report("7:", f7());
  await report("8:", f8());
  await report("9:", f9());
  await report("10:", f10());
  await report("11:", f11());
  await report("12:", f12());
  await report("13:", new C13().m());
  await report("14:", f14());
  await report("15:", f15());
  await report("16:", f16());
}

main();
