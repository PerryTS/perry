// ehfree slice 1: entering a `try` records nothing; the landing pad restores
// the runtime's managed stacks by frame marks. These cases pin the control
// flow that used to be balanced by `js_try_end` calls on every exit:
// nested try/finally with a throw at each level (design §5 test 1),
// return/break/continue through finally (test 2), and throws from finally,
// from a catch-with-finally and from a finally-only try (test 3).

const log: string[] = [];

function level3(n: number): void {
  try {
    if (n === 3) throw new Error("L3");
    log.push("body3");
  } finally {
    log.push("finally3");
  }
}

function level2(n: number): void {
  try {
    level3(n);
    if (n === 2) throw new Error("L2");
    log.push("body2");
  } finally {
    log.push("finally2");
  }
}

function level1(n: number): void {
  try {
    level2(n);
    if (n === 1) throw new Error("L1");
    log.push("body1");
  } catch (e) {
    log.push("catch1:" + (e as Error).message);
  } finally {
    log.push("finally1");
  }
}

for (const n of [0, 1, 2, 3]) {
  log.length = 0;
  level1(n);
  console.log("1 throw at", n, ":", log.join(","));
}

// 2. return through finally; a throw from the finally copy inlined before the
// `return` is NOT caught by the same try's catch.
function returnThrough(): string {
  try {
    try {
      return "ret";
    } finally {
      log.push("inner finally ran");
    }
  } catch (e) {
    return "WRONG";
  }
}

function finallyCopyBeforeReturn(): string {
  try {
    try {
      return "inner";
    } catch (e) {
      return "WRONG: the same try's catch saw its finally's throw";
    } finally {
      throw new Error("finally copy");
    }
  } catch (e) {
    return "outer caught: " + (e as Error).message;
  }
}

log.length = 0;
console.log("2", returnThrough(), log.join(","));
console.log("2", finallyCopyBeforeReturn());

function breakContinueThroughFinally(): string {
  const seen: string[] = [];
  outer: for (let i = 0; i < 4; i++) {
    for (let j = 0; j < 4; j++) {
      try {
        try {
          if (j === 1) continue;
          if (j === 2 && i === 1) continue outer;
          if (j === 3) break;
          if (i === 3) break outer;
          seen.push(`b${i}${j}`);
        } finally {
          seen.push(`f${i}${j}`);
        }
      } catch {
        seen.push("WRONG");
      }
    }
  }
  return seen.join(" ");
}
console.log("2 loops:", breakContinueThroughFinally());

// The loops above left every handler they entered; a throw now must still
// find the right one.
function afterLoops(): string {
  try {
    breakContinueThroughFinally();
    throw new Error("after loops");
  } catch (e) {
    return "caught " + (e as Error).message;
  }
}
console.log("2", afterLoops());

// 3. Throws from finally, catch-with-finally, finally-only.
function finallyReplaces(): string {
  try {
    try {
      throw new Error("first");
    } finally {
      throw new Error("second");
    }
  } catch (e) {
    return (e as Error).message;
  }
}

function catchThrowsWithFinally(): string {
  const order: string[] = [];
  try {
    try {
      throw new Error("try");
    } catch (e) {
      order.push("catch:" + (e as Error).message);
      throw new Error("from catch");
    } finally {
      order.push("finally");
    }
  } catch (e) {
    order.push("outer:" + (e as Error).message);
  }
  return order.join(",");
}

function finallyOnly(): string {
  const order: string[] = [];
  try {
    try {
      order.push("body");
      throw new Error("body throw");
    } finally {
      order.push("finally");
    }
  } catch (e) {
    order.push("outer:" + (e as Error).message);
  }
  return order.join(",");
}

console.log("3", finallyReplaces());
console.log("3", catchThrowsWithFinally());
console.log("3", finallyOnly());

// Thousands of caught throws through runtime frames that hold managed
// state: every one must be restored, or a stack grows (or the method-depth
// guard wedges after 512 leaked levels, #5591).
const u = new Uint8Array([10, 20]);
let caught = 0;
for (let i = 0; i < 3000; i++) {
  try {
    (u as any).every(NaN);
  } catch (e) {
    if (e instanceof TypeError) caught++;
  }
}
console.log("4 caught", caught, "every", (u as any).every((x: number) => x >= 10));

const m = new Map<number, number>();
for (let i = 0; i < 64; i++) m.set(i, i);
let walksAbandoned = 0;
for (let round = 0; round < 200; round++) {
  try {
    m.forEach((v) => {
      if (v === round % 64) throw new Error("stop");
    });
  } catch {
    walksAbandoned++;
  }
}
for (let i = 0; i < 60; i++) m.delete(i);
let sum = 0;
m.forEach((v) => (sum += v));
console.log("4 map walks abandoned", walksAbandoned, "size", m.size, "sum", sum);
