// #6354: a per-iteration `let` that a closure WRITES *and* that is still read
// after an `await` in the same loop body used to collapse onto one binding —
// every closure observed the last iteration's value (silent wrong answer,
// exit 0).
//
// This is the residual left by #6345: its snapshot only copies READ-ONLY
// captures (a value snapshot would drop a later write), so a written binding
// stayed in `mutable_captures`, kept its single activation-wide box, and
// collapsed. The fix backs such a binding with a one-element heap cell so the
// binding VARIABLE is a per-iteration read-only reference (snapshotted per
// iteration) while writes go to the shared element.
//
// Trip counts are 9 on purpose: the static-loop unroller (MAX_TRIP_COUNT = 8)
// mints fresh ids per unrolled copy and would mask the bug entirely at <= 8.
// Every binding here is iteration-DEPENDENT (`= i`, not `= 0`) so a collapse to
// the last value is distinguishable from the correct per-iteration answer.

const out: string[] = [];
const log = (...a: unknown[]) => out.push(a.join(" "));
const tick = () => new Promise<void>((r) => r());

// --- the reported repro: closure writes `acc`, read after the suspend ---------
async function closureWrite() {
  const fns: (() => void)[] = [];
  for (let i = 0; i < 9; i++) {
    let acc = i;
    const bump = () => {
      acc += 100;
    };
    bump();
    await tick();
    fns.push(() => log("closureWrite", acc));
  }
  fns.forEach((f) => f());
}

// --- closure writes via ++/-- (Update, not a compound LocalSet) ---------------
async function closureUpdate() {
  const fns: (() => void)[] = [];
  for (let i = 0; i < 9; i++) {
    let n = i * 10;
    const inc = () => {
      n++;
    };
    inc();
    inc();
    await tick();
    fns.push(() => log("closureUpdate", n));
  }
  fns.forEach((f) => f());
}

// --- the enclosing scope writes the binding after a closure captured it, ------
// with the await INSIDE the loop body (so the binding is live across the
// suspend and cannot be un-hoisted).
async function outerWriteAcrossAwait() {
  const fns: (() => void)[] = [];
  for (let i = 0; i < 9; i++) {
    let z = i;
    fns.push(() => log("outerWriteAcrossAwait", z));
    z += 100;
    await tick();
  }
  fns.forEach((f) => f());
}

// --- write-sharing must survive: two closures over the same binding, and a ----
// write that happens AFTER the suspend must be visible to a reader closure
// created BEFORE it (a value snapshot would break this).
async function sharedWriteAfterSuspend() {
  const readers: (() => void)[] = [];
  for (let i = 0; i < 9; i++) {
    let acc = i;
    const bump = () => {
      acc += 100;
    };
    const reader = () => log("sharedWriteAfterSuspend", acc);
    bump();
    await tick();
    bump(); // second write, AFTER the suspend
    readers.push(reader);
  }
  readers.forEach((f) => f());
}

// --- two independent written bindings in one loop body ------------------------
async function twoBindings() {
  const fns: (() => void)[] = [];
  for (let i = 0; i < 9; i++) {
    let a = i;
    let b = i * 100;
    const bump = () => {
      a += 1;
      b += 1;
    };
    bump();
    await tick();
    fns.push(() => log("twoBindings", a, b));
  }
  fns.forEach((f) => f());
}

// --- while loop ---------------------------------------------------------------
async function whileLoop() {
  const fns: (() => void)[] = [];
  let i = 0;
  while (i < 9) {
    let acc = i;
    const bump = () => {
      acc += 100;
    };
    bump();
    await tick();
    fns.push(() => log("whileLoop", acc));
    i++;
  }
  fns.forEach((f) => f());
}

// --- do/while loop ------------------------------------------------------------
async function doWhileLoop() {
  const fns: (() => void)[] = [];
  let i = 0;
  do {
    let acc = i;
    const bump = () => {
      acc += 100;
    };
    bump();
    await tick();
    fns.push(() => log("doWhileLoop", acc));
    i++;
  } while (i < 9);
  fns.forEach((f) => f());
}

// --- for-of loop --------------------------------------------------------------
async function forOfLoop() {
  const fns: (() => void)[] = [];
  for (const x of [0, 1, 2, 3, 4, 5, 6, 7, 8]) {
    let acc = x;
    const bump = () => {
      acc += 100;
    };
    bump();
    await tick();
    fns.push(() => log("forOfLoop", acc));
  }
  fns.forEach((f) => f());
}

// --- nested loops: the inner binding is per (i, j) ----------------------------
async function nestedLoops() {
  const fns: (() => void)[] = [];
  for (let i = 0; i < 3; i++) {
    for (let j = 0; j < 3; j++) {
      let s = i * 10 + j;
      const bump = () => {
        s += 100;
      };
      bump();
      await tick();
      fns.push(() => log("nestedLoops", s));
    }
  }
  fns.forEach((f) => f());
}

// --- a `var` (function-scoped) must NOT be turned per-iteration: node reports --
// the last value, and so must perry. This guards against over-application.
async function varStaysCollapsed() {
  const fns: (() => void)[] = [];
  for (let i = 0; i < 9; i++) {
    var acc = i;
    const bump = () => {
      acc += 100;
    };
    bump();
    await tick();
    fns.push(() => log("varStaysCollapsed", acc));
  }
  fns.forEach((f) => f());
}

// --- the written binding is read only inside a closure PARAM DEFAULT ----------
// (a closure default is evaluated in the enclosing scope; the rewrite must
// reach it too, not just the closure body).
async function paramDefaultCapture() {
  const fns: (() => number)[] = [];
  for (let i = 0; i < 9; i++) {
    let acc = i;
    const bump = () => {
      acc += 100;
    };
    bump();
    await tick();
    const reader = (x = acc) => x; // `acc` referenced in a param DEFAULT
    fns.push(() => reader());
  }
  for (const f of fns) log("paramDefaultCapture", f());
}

// --- the enclosing scope writes the binding AFTER the suspend -----------------
async function outerWriteAfterAwait() {
  const fns: (() => void)[] = [];
  for (let i = 0; i < 9; i++) {
    let acc = i;
    fns.push(() => log("outerWriteAfterAwait", acc));
    await tick();
    acc += 100; // written AFTER the suspend, in the enclosing scope
  }
  fns.forEach((f) => f());
}

// --- sync generator variant: a written binding live across a `yield` ----------
function* syncGen() {
  const fns: (() => void)[] = [];
  for (let i = 0; i < 9; i++) {
    let acc = i;
    const bump = () => {
      acc += 100;
    };
    bump();
    yield i;
    fns.push(() => log("syncGen", acc));
  }
  fns.forEach((f) => f());
}

// --- async generator variant: same residual, driven by the same machinery -----
async function* asyncGen() {
  const fns: (() => void)[] = [];
  for (let i = 0; i < 9; i++) {
    let acc = i;
    const bump = () => {
      acc += 100;
    };
    bump();
    await tick();
    yield i;
    fns.push(() => log("asyncGen", acc));
  }
  fns.forEach((f) => f());
}

async function main() {
  await closureWrite();
  await closureUpdate();
  await outerWriteAcrossAwait();
  await sharedWriteAfterSuspend();
  await twoBindings();
  await whileLoop();
  await doWhileLoop();
  await forOfLoop();
  await nestedLoops();
  await paramDefaultCapture();
  await outerWriteAfterAwait();
  await varStaysCollapsed();
  for (const _ of syncGen()) {
    // drain
  }
  for await (const _ of asyncGen()) {
    // drain
  }

  for (const line of out) console.log(line);
}

main();
