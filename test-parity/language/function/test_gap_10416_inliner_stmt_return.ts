// #10416: a small helper called as a STATEMENT (result discarded) whose last
// statement is an `if` containing `return` had that `return` spliced into the
// caller by the HIR inliner, so the caller returned the helper's value
// (decimal.js `pow()` returned `true`), and at module top level the stray
// return produced invalid LLVM IR. Every caller below must return its own
// value; each helper's side effects must happen exactly as in Node.

// ---- callees ---------------------------------------------------------------

// `if (c) { return v; }` as the only statement.
function onlyGuard(x: number) {
  if (x > 1) {
    return "only";
  }
}

// decimal.js `truncate`: `if` with a side effect and `return true` last.
function truncate(arr: number[], len: number) {
  if (arr.length > len) {
    arr.length = len;
    return true;
  }
}

// `const y = …; if (y) return v;`
function constGuard(n: number) {
  const y = n % 2 === 0;
  if (y) return "even";
}

// if/else with a return in each branch.
function bothBranches(x: number) {
  if (x > 1) {
    return "a";
  } else {
    return "b";
  }
}

// Early return, then more work (return not in the last statement).
function guardThenWork(log: string[], x: number) {
  if (x < 0) {
    log.push("neg" + x);
    return "neg";
  }
  log.push("work" + x);
  return "done";
}

// The else branch returns, the then branch falls through to the tail.
function elseReturns(log: string[], x: number) {
  if (x % 2 === 0) {
    log.push("even" + x);
  } else {
    return "odd";
  }
  log.push("after" + x);
}

// A conditional return nested in a branch that also falls through.
function nestedFallthrough(log: string[], x: number) {
  if (x > 0) {
    if (x > 5) {
      return "big";
    }
    log.push("small" + x);
  }
  log.push("tail" + x);
}

// `return;` with no value.
function voidGuard(log: string[], x: number) {
  if (x === 3) return;
  log.push("v" + x);
}

// Controls that were already correct.
function guardThenReturn(x: number) {
  if (x > 1) {
    return "v";
  }
  return "w";
}

function guardThenThrow(x: number) {
  if (x > 1) {
    return "v";
  }
  throw new Error("small " + x);
}

function twice(y: number) {
  const z = y * 2;
  return z + 1;
}

// ---- call statements inside loops ------------------------------------------
// Loop bounds come from parameters so the loops stay loops.

function inFor(n: number): string {
  const arr = [1, 2, 3, 4, 5];
  for (let i = 0; i < n; i++) {
    truncate(arr, 4 - i);
    onlyGuard(i + 1);
  }
  return "inFor:" + arr.join(",");
}

function inWhile(n: number): string {
  const log: string[] = [];
  let i = 0;
  while (i < n) {
    constGuard(i);
    bothBranches(i);
    elseReturns(log, i);
    i++;
  }
  return "inWhile:" + i + ":" + log.join(",");
}

function inDoWhile(n: number): string {
  const log: string[] = [];
  let i = -2;
  do {
    guardThenWork(log, i);
    voidGuard(log, i + 5);
    constGuard(i);
    i++;
  } while (i < n);
  return "inDoWhile:" + log.join(",");
}

// decimal.js `intPow` shape: `truncate(…)` as a statement inside `for (;;)`.
function intPowLike(k: number): string {
  const digits = [9, 9, 9, 9, 9, 9, 9, 9];
  let n = 13;
  let rounds = 0;
  for (;;) {
    rounds++;
    if (n % 2) {
      truncate(digits, k + rounds);
    }
    n = Math.floor(n / 2);
    if (n === 0) break;
    truncate(digits, k + 4);
  }
  return "intPow:" + rounds + ":" + digits.length;
}

function nestedInLoop(n: number): string {
  const log: string[] = [];
  for (let i = 0; i < n; i += 3) {
    nestedFallthrough(log, i);
    // The call is kept here; the call nested in its argument is still inlined.
    nestedFallthrough(log, twice(i));
  }
  return "nested:" + log.join(",");
}

// ---- other caller shapes ---------------------------------------------------

// More than 10 statements, so the caller is not itself an inline candidate.
function manyStatements(): string {
  const log: string[] = [];
  log.push("s1");
  onlyGuard(5);
  log.push("s2");
  bothBranches(0);
  log.push("s3");
  constGuard(8);
  log.push("s4");
  guardThenWork(log, -1);
  guardThenWork(log, 1);
  elseReturns(log, 2);
  elseReturns(log, 3);
  voidGuard(log, 3);
  voidGuard(log, 4);
  log.push("s5");
  return "many:" + log.join(",");
}

class Store {
  items: number[] = [];
  log: string[] = [];

  add(v: number) {
    if (v < 0) {
      return false;
    }
    this.items.push(v);
  }

  trimTo(n: number) {
    if (this.items.length > n) {
      this.items.length = n;
      return true;
    }
  }

  withLoop(n: number): string {
    for (let i = 0; i < n; i++) {
      onlyGuard(5);
      guardThenWork(this.log, i - 1);
    }
    return "withLoop:" + this.log.join(",");
  }

  noLoop(): string {
    bothBranches(3);
    constGuard(2);
    return "noLoop";
  }
}

class Gate {
  open = 0;
  check(n: number) {
    if (n > 2) {
      this.open = n;
      return "opened";
    }
  }
}

// Method callees on an exact receiver.
function methodCallee(n: number): string {
  const g = new Gate();
  g.check(5);
  const s = new Store();
  for (let i = -2; i < n; i++) {
    s.add(i);
    s.trimTo(3);
  }
  return "method:" + g.open + ":" + s.items.join(",");
}

// Unreachable code after `return;` in a method used as a value.
class Dead {
  x = 0;
  m() {
    return;
    this.x = 1;
  }
}

function deadAfterReturn(): string {
  const d = new Dead();
  console.log("dead m():", d.m());
  for (let i = 0; i < 1; i++) {}
  return "dead:" + d.x;
}

function inArrowInsideFunction(): string {
  const arr = [1, 2, 3];
  const run = (): string => {
    truncate(arr, 1);
    constGuard(4);
    return "arrow-in-fn:" + arr.length;
  };
  return run();
}

const topArrow = (): string => {
  constGuard(2);
  onlyGuard(9);
  bothBranches(9);
  return "top-arrow";
};

const topArrowLoop = (n: number): string => {
  let hits = 0;
  for (let i = 0; i < n; i++) {
    onlyGuard(i);
    hits++;
  }
  return "top-arrow-loop:" + hits;
};

// ---- early returns before more statements, in other caller contexts -----------

// `var` declared before the return is still visible (undefined) afterwards.
function varBeforeReturn(log: string[], x: number) {
  if (x > 0) {
    var y = "set" + x;
    return;
  }
  log.push("y=" + y);
}

function effectReturn(log: string[], c: boolean) {
  if (c) return log.push("ret-effect");
  log.push("no-ret");
}

function inner(log: string[], x: number) {
  if (x === 2) return "two";
  log.push("inner" + x);
}

function outer(log: string[], x: number) {
  if (x === 0) return "zero";
  inner(log, x);
  log.push("outer" + x);
}

function throwOrReturn(log: string[], x: number) {
  if (x > 10) {
    throw new Error("big" + x);
  } else if (x > 5) {
    return "mid";
  }
  log.push("low" + x);
}

function controlFlowCaller(n: number): string {
  const log: string[] = [];
  for (let i = -1; i < n; i++) {
    varBeforeReturn(log, i);
    effectReturn(log, i === 0);
    outer(log, i);
    switch (i) {
      case 1:
        inner(log, 5);
        break;
      default:
        inner(log, 2);
    }
    try {
      throwOrReturn(log, i * 6);
    } catch (e) {
      log.push((e as Error).message);
    } finally {
      inner(log, i + 100);
    }
  }
  return "control-flow:" + log.join(",");
}

function* generatorCaller(n: number) {
  const log: string[] = [];
  for (let i = 0; i < n; i++) {
    outer(log, i);
    yield log.length;
    onlyGuard(i);
    inner(log, i);
  }
  return log.join(",");
}

async function asyncCaller(n: number): Promise<string> {
  const log: string[] = [];
  for (let i = 0; i < n; i++) {
    outer(log, i);
    await Promise.resolve(i);
    constGuard(i);
    inner(log, i);
  }
  return "async:" + log.join(",");
}

// ---- controls ----------------------------------------------------------------

function controls(): string {
  const out: string[] = [];
  let r: string | undefined = onlyGuard(5);
  out.push(String(r));
  r = onlyGuard(0);
  out.push(String(r));
  const b = bothBranches(0);
  out.push(b);
  out.push(guardThenReturn(5), guardThenReturn(0));
  guardThenReturn(5);
  guardThenReturn(0);
  out.push(guardThenThrow(5));
  guardThenThrow(5);
  try {
    guardThenThrow(0);
  } catch (e) {
    out.push((e as Error).message);
  }
  onlyGuard(0);
  for (let i = 0; i < 1; i++) {}
  return "controls:" + out.join(",");
}

console.log(inFor(3));
console.log(inWhile(4));
console.log(inDoWhile(2));
console.log(intPowLike(2));
console.log(nestedInLoop(8));
console.log(manyStatements());
const store = new Store();
console.log(store.withLoop(3));
console.log(store.noLoop());
console.log(methodCallee(6));
console.log(deadAfterReturn());
console.log(inArrowInsideFunction());
console.log(topArrow());
console.log(topArrowLoop(3));
console.log(controls());
console.log(controlFlowCaller(3));
const gen = generatorCaller(3);
const yields: string[] = [];
let step = gen.next();
while (!step.done) {
  yields.push(String(step.value));
  step = gen.next();
}
console.log("generator:" + yields.join(",") + ":" + step.value);
asyncCaller(3).then((s) => console.log(s));

// Module top level: must compile and keep running past each call.
onlyGuard(5);
bothBranches(0);
constGuard(4);
truncate([1, 2, 3], 1);
console.log("toplevel-after");
