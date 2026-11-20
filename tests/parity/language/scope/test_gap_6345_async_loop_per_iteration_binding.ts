// #6345: a `let`/`const` declared in a loop body inside an ASYNC function must
// keep its per-iteration binding. The async-to-generator transform used to
// hoist every body local into one activation-wide box, so every closure made in
// the loop observed the LAST iteration's value (silent wrong answer, exit 0).
//
// Trip counts are 9 on purpose: the static-loop unroller (MAX_TRIP_COUNT = 8)
// mints fresh ids per unrolled copy and would mask the bug entirely at <= 8.

const out: string[] = [];
const log = (...a: unknown[]) => out.push(a.join(" "));
const tick = () => new Promise<void>((r) => r());

// --- the reported repro: `const j = i` captured, await AFTER the loop --------
async function constCapture() {
  const fns: (() => void)[] = [];
  for (let i = 0; i < 9; i++) {
    const j = i;
    fns.push(() => log("constCapture", j));
  }
  fns.forEach((f) => f());
  await 0;
}

// --- capturing the loop variable directly ------------------------------------
async function loopVarCapture() {
  const fns: (() => void)[] = [];
  for (let i = 0; i < 9; i++) {
    fns.push(() => log("loopVarCapture", i));
  }
  fns.forEach((f) => f());
  await 0;
}

// --- await INSIDE the body, binding declared BEFORE the await -----------------
// (the capture has to survive the suspend/resume)
async function awaitInBodyAfterCapture() {
  const fns: (() => void)[] = [];
  for (let i = 0; i < 9; i++) {
    const j = i;
    fns.push(() => log("awaitInBodyAfterCapture", j));
    await tick();
  }
  fns.forEach((f) => f());
}

// --- await INSIDE the body, binding declared AFTER the await ------------------
async function awaitInBodyBeforeCapture() {
  const fns: (() => void)[] = [];
  for (let i = 0; i < 9; i++) {
    const j = i;
    await tick();
    fns.push(() => log("awaitInBodyBeforeCapture", j));
  }
  fns.forEach((f) => f());
}

// --- loop variable captured, await inside the body ---------------------------
async function loopVarCaptureWithAwait() {
  const fns: (() => void)[] = [];
  for (let i = 0; i < 9; i++) {
    fns.push(() => log("loopVarCaptureWithAwait", i));
    await tick();
  }
  fns.forEach((f) => f());
}

// --- while / do-while ---------------------------------------------------------
async function whileLoop() {
  const fns: (() => void)[] = [];
  let i = 0;
  while (i < 9) {
    const j = i;
    fns.push(() => log("whileLoop", j));
    i++;
  }
  fns.forEach((f) => f());
  await 0;
}

async function doWhileLoop() {
  const fns: (() => void)[] = [];
  let i = 0;
  do {
    const j = i;
    fns.push(() => log("doWhileLoop", j));
    i++;
  } while (i < 9);
  fns.forEach((f) => f());
  await 0;
}

// --- for-of / for-in / for-await-of ------------------------------------------
async function forOf() {
  const fns: (() => void)[] = [];
  for (const x of [10, 20, 30, 40, 50, 60, 70, 80, 90]) {
    fns.push(() => log("forOf", x));
  }
  fns.forEach((f) => f());
  await 0;
}

async function forIn() {
  const fns: (() => void)[] = [];
  for (const k in { a: 1, b: 2, c: 3, d: 4, e: 5, f: 6, g: 7, h: 8, i: 9 }) {
    fns.push(() => log("forIn", k));
  }
  fns.forEach((f) => f());
  await 0;
}

async function forAwaitOf() {
  const fns: (() => void)[] = [];
  for await (const x of [1, 2, 3, 4, 5, 6, 7, 8, 9]) {
    const y = x * 10;
    fns.push(() => log("forAwaitOf", y));
  }
  fns.forEach((f) => f());
}

// --- nested loops, and one closure capturing BOTH levels ----------------------
async function nested() {
  const fns: (() => void)[] = [];
  for (let a = 0; a < 3; a++) {
    for (let b = 0; b < 3; b++) {
      fns.push(() => log("nested", a, b));
    }
  }
  fns.forEach((f) => f());
  await 0;
}

async function nestedAwaitInInner() {
  const fns: (() => void)[] = [];
  for (let a = 0; a < 3; a++) {
    for (let b = 0; b < 3; b++) {
      fns.push(() => log("nestedAwaitInInner", a, b));
      await tick();
    }
  }
  fns.forEach((f) => f());
}

// --- `var` must STAY function-scoped: ONE binding, closures see the last value
async function varStaysVar() {
  const fns: (() => void)[] = [];
  for (var i = 0; i < 9; i++) {
    var v = i;
    fns.push(() => log("varStaysVar", v));
  }
  fns.forEach((f) => f());
  log("varStaysVar after", v, i);
  await 0;
}

// --- a binding the closure WRITES keeps its shared box (no snapshot) ----------
async function closureWritesBinding() {
  const fns: (() => void)[] = [];
  for (let i = 0; i < 9; i++) {
    let acc = 0;
    const add = () => {
      acc += 5;
    };
    add();
    await tick();
    fns.push(() => log("closureWritesBinding", acc));
  }
  fns.forEach((f) => f());
}

// --- outer scope mutates the binding AFTER the closure captured it ------------
// (same binding, so the closure must observe the update)
async function outerWritesAfterCapture() {
  const fns: (() => void)[] = [];
  for (let i = 0; i < 9; i++) {
    let z = i;
    fns.push(() => log("outerWritesAfterCapture", z));
    z += 100;
  }
  fns.forEach((f) => f());
  await 0;
}

// --- labeled loop + continue --------------------------------------------------
async function labeledLoop() {
  const fns: (() => void)[] = [];
  outer: for (let i = 0; i < 9; i++) {
    if (i === 3) continue outer;
    const j = i;
    await tick();
    fns.push(() => log("labeledLoop", j));
  }
  fns.forEach((f) => f());
}

// --- try / catch / finally inside a suspending loop ---------------------------
async function tryCatchInLoop() {
  const fns: (() => void)[] = [];
  for (let i = 0; i < 5; i++) {
    try {
      const j = i;
      await tick();
      if (i === 2) throw new Error("boom");
      fns.push(() => log("tryCatchInLoop ok", j));
    } catch {
      const k = i;
      fns.push(() => log("tryCatchInLoop err", k));
    } finally {
      const f = i;
      fns.push(() => log("tryCatchInLoop fin", f));
    }
  }
  fns.forEach((f) => f());
}

// --- switch inside a suspending loop ------------------------------------------
async function switchInLoop() {
  const fns: (() => void)[] = [];
  for (let i = 0; i < 9; i++) {
    switch (i % 2) {
      case 0: {
        const e = i;
        await tick();
        fns.push(() => log("switchInLoop even", e));
        break;
      }
      default: {
        const o = i;
        fns.push(() => log("switchInLoop odd", o));
      }
    }
  }
  fns.forEach((f) => f());
}

// --- a captured ARRAY binding mutated from inside the closure ------------------
// (exercises the id rewrite on array fast-path expressions)
async function arrayBindingCapture() {
  const fns: (() => void)[] = [];
  for (let i = 0; i < 9; i++) {
    const row: number[] = [];
    fns.push(() => {
      row.push(i);
      log("arrayBindingCapture", i, row.length, row[0]);
    });
    await tick();
  }
  fns.forEach((f) => f());
}

// --- destructured binding in a suspending loop ---------------------------------
async function destructuredBinding() {
  const fns: (() => void)[] = [];
  for (const o of [{ v: 1 }, { v: 2 }, { v: 3 }, { v: 4 }, { v: 5 }]) {
    const { v } = o;
    await tick();
    fns.push(() => log("destructuredBinding", v));
  }
  fns.forEach((f) => f());
}

// --- a closure nested inside a closure, capturing two loop levels ---------------
async function nestedClosureCapture() {
  const fns: (() => void)[] = [];
  for (let a = 0; a < 3; a++) {
    for (let b = 0; b < 3; b++) {
      fns.push(() => {
        const inner = () => log("nestedClosureCapture", a, b);
        inner();
      });
      await tick();
    }
  }
  fns.forEach((f) => f());
}

// --- SYNC generator: same state-machine lowering, no async ----------------------
function* syncGenerator(): Generator<number> {
  const fns: (() => number)[] = [];
  for (let i = 0; i < 9; i++) {
    const j = i;
    fns.push(() => j);
    yield i;
  }
  for (const f of fns) yield f();
}

// --- non-async control: must be unchanged --------------------------------------
function plainFunction() {
  const fns: (() => void)[] = [];
  for (let i = 0; i < 9; i++) {
    const j = i;
    fns.push(() => log("plainFunction", j));
  }
  fns.forEach((f) => f());
}

async function main() {
  await constCapture();
  await loopVarCapture();
  await awaitInBodyAfterCapture();
  await awaitInBodyBeforeCapture();
  await loopVarCaptureWithAwait();
  await whileLoop();
  await doWhileLoop();
  await forOf();
  await forIn();
  await forAwaitOf();
  await nested();
  await nestedAwaitInInner();
  await varStaysVar();
  await closureWritesBinding();
  await outerWritesAfterCapture();
  await labeledLoop();
  await tryCatchInLoop();
  await switchInLoop();
  await arrayBindingCapture();
  await destructuredBinding();
  await nestedClosureCapture();
  for (const v of syncGenerator()) log("syncGenerator", v);
  plainFunction();

  for (const line of out) console.log(line);
}

main();
