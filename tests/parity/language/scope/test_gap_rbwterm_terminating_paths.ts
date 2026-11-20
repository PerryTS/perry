// A `let x;` (or hoisted `var`) written on every path that continues past a
// switch / if / try is definitely assigned there: paths ending in return,
// throw, break or continue do not reach the read. Each case below must print
// what node prints, both where the binding is assigned on every continuing
// path and where some path still reaches the read with `undefined`.

// switch with a throwing default (parsePseudoBigInt's log2Base).
function log2Base(c: number, x: number): number {
  let b;
  switch (c) {
    case 98:
      b = x | 0;
      break;
    case 111:
      b = (x + 2) | 0;
      break;
    case 120:
      b = (x + 3) | 0;
      break;
    default:
      throw new Error("bad base " + c);
  }
  b = (b * 2) | 0;
  return b + 1;
}
console.log("log2Base", log2Base(98, 1), log2Base(111, 1), log2Base(120, 1));
try {
  log2Base(5, 1);
} catch (e) {
  console.log("log2Base threw", (e as Error).message);
}

// switch with a returning default and a Number-typed local.
function returningDefault(c: number): number {
  let r;
  switch (c) {
    case 1:
      r = c * 2.5;
      break;
    case 2:
      r = c * 0.5;
      break;
    default:
      return -1;
  }
  r = r * 2;
  return r + 1;
}
console.log("returningDefault", returningDefault(1), returningDefault(2), returningDefault(3));

// A missing default: no case taken leaves the seed, so the read is NaN.
function missingDefault(c: number, x: number): number {
  let b;
  switch (c) {
    case 1:
      b = x | 0;
      break;
    case 2:
      b = (x + 1) | 0;
      break;
  }
  return b + 1;
}
console.log("missingDefault", missingDefault(1, 4), missingDefault(2, 4), missingDefault(3, 4));

function missingDefaultNumber(c: number): number {
  let r;
  switch (c) {
    case 1:
      r = c * 1.5;
      break;
  }
  return r * 2;
}
console.log("missingDefaultNumber", missingDefaultNumber(1), missingDefaultNumber(2));

// A break that skips the write reaches the code after the switch.
function breakSkipsWrite(c: number, x: number): number {
  let b;
  switch (c) {
    case 1:
      break;
    default:
      b = x | 0;
  }
  return b + 1;
}
console.log("breakSkipsWrite", breakSkipsWrite(1, 4), breakSkipsWrite(2, 4));

// Fallthrough: empty cases fall into a writing one; a write above falls
// into a reading default, which is also entered directly.
function fallthrough(c: number, x: number): string {
  let b;
  let r = 0;
  switch (c) {
    case 1:
    case 2:
      b = x | 0;
      break;
    case 3:
      b = (x * 3) | 0;
    // falls through
    default:
      r = b + 1;
      b = 0;
  }
  return b + ":" + r;
}
console.log("fallthrough", fallthrough(1, 4), fallthrough(3, 4), fallthrough(9, 4));

// The default sits above the last case, which completes the switch.
function defaultFirst(c: number, x: number): number {
  let b;
  switch (c) {
    default:
      throw new Error("no");
    case 7:
      b = x | 0;
  }
  return b + 1;
}
console.log("defaultFirst", defaultFirst(7, 4));

// Labeled continue / break out of a loop from inside a switch.
function labeledContinue(m: number): number {
  let total = 0;
  outer: for (let i = 0; i < m; i++) {
    let v;
    switch (i % 3) {
      case 0:
        v = i | 0;
        break;
      case 1:
        v = (i * 2) | 0;
        break;
      default:
        continue outer;
    }
    total = (total + v) | 0;
  }
  return total;
}
console.log("labeledContinue", labeledContinue(10));

function labeledBreak(k: number): number {
  let v;
  outer: for (let i = 0; i < 3; i++) {
    switch (k) {
      case 1:
        v = (10 + i) | 0;
        break;
      default:
        break outer;
    }
    v = (v + i) | 0;
  }
  return v + 1;
}
console.log("labeledBreak", labeledBreak(1), labeledBreak(2));

// if / else with one returning branch.
function ifReturn(k: number): number {
  let v;
  if (k > 5) {
    return -1;
  } else {
    v = (k * 2) | 0;
  }
  return v + 1;
}
console.log("ifReturn", ifReturn(2), ifReturn(9));

function ifNoReturn(k: number): number {
  let v;
  if (k > 5) {
    k = k + 1;
  } else {
    v = (k * 2) | 0;
  }
  return v + 1;
}
console.log("ifNoReturn", ifNoReturn(2), ifNoReturn(9));

// try / catch where the catch rethrows: the read only follows the block.
function parseDigit(s: string): number {
  const d = s.charCodeAt(0) - 48;
  if (d < 0 || d > 9) throw new Error("nan " + s);
  return d;
}
function tryRethrow(s: string): number {
  let v;
  try {
    v = parseDigit(s) | 0;
  } catch (e) {
    throw e;
  }
  return v + 1;
}
console.log("tryRethrow", tryRethrow("7"));
try {
  tryRethrow("x");
} catch (e) {
  console.log("tryRethrow threw", (e as Error).message);
}

// The catch completes: the read after it can see the seed.
function trySwallow(s: string): number {
  let v;
  try {
    v = parseDigit(s) | 0;
  } catch (e) {
    // swallowed
  }
  return v + 1;
}
console.log("trySwallow", trySwallow("7"), trySwallow("x"));

// try / finally with a returning finally: nothing continues past the try.
function finallyReturn(k: number): number {
  let v;
  if (k) {
    try {
      if (k > 1) throw new Error("x");
    } finally {
      return 100 + k;
    }
  } else {
    v = 5 | 0;
  }
  return v + 1;
}
console.log("finallyReturn", finallyReturn(0), finallyReturn(1), finallyReturn(2));

// A completing finally does not cover a catch path that skipped the write.
function finallyCompletes(s: string): number {
  let v;
  let n = 0;
  try {
    v = parseDigit(s) | 0;
  } catch (e) {
    n = 1;
  } finally {
    n = n + 10;
  }
  return v + n;
}
console.log("finallyCompletes", finallyCompletes("3"), finallyCompletes("x"));

// Nested functions: each body is judged on its own.
function outerFn(k: number): number {
  function inner(c: number): number {
    let w;
    switch (c) {
      case 1:
        w = 7 | 0;
        break;
      default:
        throw new Error("inner");
    }
    return w * 3;
  }
  let r;
  switch (k) {
    case 0:
      r = inner(1) | 0;
      break;
    case 1:
      r = -1;
      break;
  }
  return r + 1;
}
console.log("outerFn", outerFn(0), outerFn(1), outerFn(2));

// break inside a switch inside a loop leaves only the switch.
function loopSwitch(m: number): number {
  let s = 0;
  for (let i = 0; i < m; i++) {
    let v;
    switch (i & 1) {
      case 0:
        v = i | 0;
        break;
      default:
        v = -i | 0;
    }
    s = (s + v) | 0;
  }
  return s;
}
console.log("loopSwitch", loopSwitch(7));

function loopSwitchSkip(m: number): number {
  let s = 0;
  for (let i = 0; i < m; i++) {
    let v;
    switch (i & 1) {
      case 0:
        break;
      default:
        v = i | 0;
    }
    s = s + v;
  }
  return s;
}
console.log("loopSwitchSkip", loopSwitchSkip(4));

// A string-switch kind (the checker's currentKind) with a returning default.
function kindOf(tag: string): number {
  let currentKind;
  switch (tag) {
    case "get":
      currentKind = 1;
      break;
    case "set":
      currentKind = 2;
      break;
    case "prop":
      currentKind = 4;
      break;
    default:
      return 0;
  }
  return currentKind | 8;
}
console.log("kindOf", kindOf("get"), kindOf("set"), kindOf("prop"), kindOf("x"));

// A loop body is never assumed to run, even one that always runs once.
function loopOnce(x: number): number {
  let v;
  while (true) {
    v = x | 0;
    break;
  }
  return v + 1;
}
console.log("loopOnce", loopOnce(4));

// Hoisted var assigned in every continuing case, read after the switch.
function varSwitch(c: number): number {
  switch (c) {
    case 1:
      var hv = 3;
      break;
    case 2:
      hv = 4;
      break;
    default:
      return -2;
  }
  hv = hv * 2;
  return hv + 1;
}
console.log("varSwitch", varSwitch(1), varSwitch(2), varSwitch(3));
