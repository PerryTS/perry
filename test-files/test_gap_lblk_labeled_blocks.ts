// Labeled non-loop blocks (#12662, #12663). A labeled block is a target of
// `break <label>` only: an unlabeled break targets the nearest loop/switch and
// an unlabeled continue the nearest loop. A `break label` leaves the block at
// its end, and every path through it must be joined there.

// #12663: unlabeled break inside a labeled block inside a switch.
function issueSwitch(k: number): string {
  let r = "a";
  switch (k) {
    case 1:
      L: {
        if (k) break;
        r += "b";
      }
      r += "c";
  }
  return r;
}
console.log("issueSwitch", issueSwitch(1), issueSwitch(2));

// #12662: break out of a labeled block skips the numeric write. (The issue's
// `v = x | 0` form also needs the read-before-write check of the integer-local
// proof for its `let v;` seed; `x + 0.5` exercises the Number-at-read flow.)
function issueNumber(k: number, x: number): number {
  let v;
  blk: {
    if (k) break blk;
    v = x + 0.5;
  }
  return v + 1;
}
console.log("issueNumber", issueNumber(1, 3), issueNumber(0, 3));

function breakInFor(n: number): string {
  let r = "";
  for (let i = 0; i < n; i++) {
    L: {
      if (i === 2) break;
      r += i;
    }
    r += ".";
  }
  return r;
}
console.log("breakInFor", breakInFor(5));

function continueInFor(n: number): string {
  let r = "";
  for (let i = 0; i < n; i++) {
    L: {
      if (i % 2) continue;
      r += i;
    }
    r += ".";
  }
  return r;
}
console.log("continueInFor", continueInFor(5));

function inWhile(n: number): string {
  let r = "";
  let i = 0;
  while (i < n) {
    i++;
    L: {
      if (i === 2) continue;
      if (i === 4) break;
      r += i;
    }
    r += "|";
  }
  return r + "!" + i;
}
console.log("inWhile", inWhile(6));

function inDoWhile(n: number): string {
  let r = "";
  let i = 0;
  do {
    i++;
    L: {
      if (i === 1) continue;
      if (i === 3) break;
      r += i;
    }
    r += "|";
  } while (i < n);
  return r + "!" + i;
}
console.log("inDoWhile", inDoWhile(6));

function inForOf(xs: string[]): string {
  let r = "";
  for (const x of xs) {
    L: {
      if (x === "b") continue;
      if (x === "d") break;
      r += x;
    }
    r += "-";
  }
  return r;
}
console.log("inForOf", inForOf(["a", "b", "c", "d", "e"]));

// Iterator close: a break out of a labeled block in a for-of closes the iterator.
function iterClose(): string {
  let log = "";
  const it = {
    i: 0,
    [Symbol.iterator]() {
      return this;
    },
    next() {
      this.i++;
      return { value: this.i, done: this.i > 5 };
    },
    return() {
      log += "closed";
      return { value: undefined, done: true };
    },
  };
  for (const v of it) {
    L: {
      if (v === 1) continue;
      if (v === 3) break;
      log += v;
    }
  }
  return log;
}
console.log("iterClose", iterClose());

function inForIn(o: Record<string, number>): string {
  let r = "";
  for (const k in o) {
    L: {
      if (k === "b") continue;
      if (k === "d") break;
      r += k;
    }
    r += "+";
  }
  return r;
}
console.log("inForIn", inForIn({ a: 1, b: 2, c: 3, d: 4, e: 5 }));

// continue inside a labeled block inside a switch inside a loop.
function switchContinue(n: number): string {
  let r = "";
  for (let i = 0; i < n; i++) {
    switch (i) {
      case 1:
        L: {
          if (i) continue;
          r += "x";
        }
        r += "y";
        break;
      default:
        r += i;
    }
    r += ";";
  }
  return r;
}
console.log("switchContinue", switchContinue(4));

// A labeled switch: an unlabeled continue inside continues the loop.
function labeledSwitchContinue(n: number): string {
  let r = "";
  for (let i = 0; i < n; i++) {
    S: switch (i) {
      case 1:
        continue;
      case 2:
        break S;
      default:
        r += i;
    }
    r += ";";
  }
  return r;
}
console.log("labeledSwitchContinue", labeledSwitchContinue(4));

// break L from nested depth (loop and switch inside the block).
function nestedBreak(k: number): string {
  let r = "";
  L: {
    for (let i = 0; i < 3; i++) {
      switch (i) {
        case 1:
          if (k) break L;
          r += "s";
          break;
      }
      r += i;
    }
    r += "end";
  }
  return r + "!";
}
console.log("nestedBreak", nestedBreak(1), nestedBreak(0));

// Nested labeled blocks: unlabeled break crosses two blocks.
function twoBlocks(n: number): string {
  let r = "";
  for (let i = 0; i < n; i++) {
    A: {
      B: {
        if (i === 1) break B;
        if (i === 2) break A;
        if (i === 3) break;
        r += "b" + i;
      }
      r += "a" + i;
    }
    r += ",";
  }
  return r;
}
console.log("twoBlocks", twoBlocks(5));

// A labeled block inside a labeled loop: continue outer / break outer.
function outerLoop(): string {
  let r = "";
  outer: for (let i = 0; i < 4; i++) {
    for (let j = 0; j < 4; j++) {
      L: {
        if (j === 1) continue outer;
        if (i === 3) break outer;
        if (j === 0 && i === 2) continue;
        r += i + "" + j;
      }
      r += "/";
    }
    r += "never";
  }
  return r;
}
console.log("outerLoop", outerLoop());

// Label chain on a loop with a labeled block inside.
function chain(): string {
  let r = "";
  a: b: for (let i = 0; i < 4; i++) {
    L: {
      if (i === 0) continue a;
      if (i === 1) continue b;
      if (i === 3) break;
      r += i;
    }
  }
  return r;
}
console.log("chain", chain());

// try/finally inside a labeled block: break L runs the finally.
function tryFinally(k: number): string {
  let r = "";
  L: {
    try {
      if (k) break L;
      r += "t";
    } finally {
      r += "f";
    }
    r += "after";
  }
  return r + "!";
}
console.log("tryFinally", tryFinally(1), tryFinally(0));

// Unlabeled break from a try in a labeled block in a loop runs the finally.
function loopTryFinally(): string {
  let r = "";
  for (let i = 0; i < 3; i++) {
    L: {
      try {
        if (i === 1) break;
        r += i;
      } finally {
        r += "f";
      }
    }
  }
  return r;
}
console.log("loopTryFinally", loopTryFinally());

// Generators: yield inside labeled blocks.
function* gen(n: number) {
  for (let i = 0; i < n; i++) {
    L: {
      if (i === 1) continue;
      yield "a" + i;
      if (i === 3) break;
      if (i === 2) break L;
      yield "b" + i;
    }
    yield "c" + i;
  }
  M: {
    yield "m";
    break M;
  }
  yield "end";
}
console.log("gen", [...gen(6)].join(" "));

function* genSwitch(k: number) {
  switch (k) {
    case 1:
      L: {
        yield "x";
        if (k) break;
        yield "y";
      }
      yield "z";
  }
  yield "done";
}
console.log("genSwitch", [...genSwitch(1)].join(" "));

// Async: await inside labeled blocks.
async function asyncFn(n: number): Promise<string> {
  let r = "";
  for (let i = 0; i < n; i++) {
    L: {
      const v = await Promise.resolve(i);
      if (v === 1) continue;
      if (v === 3) break;
      if (v === 2) break L;
      r += "a" + v;
    }
    r += "c" + i;
  }
  K: {
    await null;
    if (n) break K;
    r += "never";
  }
  return r;
}

async function asyncSwitch(k: number): Promise<string> {
  let r = "s";
  switch (k) {
    case 1:
      L: {
        await null;
        if (k) break;
        r += "b";
      }
      r += "c";
  }
  return r;
}

// Number typing around labeled blocks.
function numberWhile(k: number, x: number): number {
  let v;
  let i = 0;
  while (i < 3) {
    i++;
    L: {
      if (k) break L;
      v = x + 0.5;
    }
  }
  return v + 1;
}

function numberUndefinedAfterBreak(x: number): number {
  let v = 1;
  let i = 0;
  while (i < 2) {
    v = undefined as any;
    if (i === 0 && x > 0) break;
    v = x | 0;
    i++;
  }
  return v + 1;
}

function numberDoWhile(k: number, x: number): number {
  let v;
  do {
    if (k) break;
    v = x + 0.5;
  } while (false);
  return v + 1;
}

function numberContinue(x: number): number {
  let v;
  let s = 0;
  for (let i = 0; i < 3; i++) {
    v = undefined as any;
    L: {
      if (i === 1) continue;
      v = x + 0.5;
    }
    s += v;
  }
  return s;
}

function numberSwitch(k: number, x: number): number {
  let v;
  switch (k) {
    case 1:
      L: {
        if (x) break;
        v = x + 0.5;
      }
      v = 7;
      break;
    default:
      v = 2;
  }
  return v + 1;
}

function numberAllPaths(k: number, x: number): number {
  let v;
  L: {
    if (k) {
      v = x + 0.5;
      break L;
    }
    v = (x + 1) + 0.5;
  }
  return v + 1;
}

function numberFinally(k: number, x: number): number {
  let v;
  L: {
    try {
      if (k) break L;
      v = x + 0.5;
    } finally {
      x = x + 1;
    }
  }
  return v + x;
}

console.log("numberWhile", numberWhile(1, 3), numberWhile(0, 3));
console.log("numberUndefinedAfterBreak", numberUndefinedAfterBreak(5), numberUndefinedAfterBreak(-1));
console.log("numberDoWhile", numberDoWhile(1, 3), numberDoWhile(0, 3));
console.log("numberContinue", numberContinue(4));
console.log("numberSwitch", numberSwitch(1, 1), numberSwitch(1, 0), numberSwitch(2, 0));
console.log("numberAllPaths", numberAllPaths(1, 4), numberAllPaths(0, 4));
console.log("numberFinally", numberFinally(1, 3), numberFinally(0, 3));

asyncFn(5)
  .then((r) => console.log("asyncFn", r))
  .then(() => asyncSwitch(1))
  .then((r) => console.log("asyncSwitch", r));
