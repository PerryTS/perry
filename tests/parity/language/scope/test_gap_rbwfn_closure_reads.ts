// A nested function declaration reads its captured locals where it can
// first run: a call to it, a call to another function that uses it, or any
// use that hands its value on (an argument, a property, a return, an
// alias). Creating it at the top of the body reads nothing. Each case must
// print what node prints, both where every run follows the write and where
// a run still sees the `undefined` a `var` starts as. A captured String
// local carries its proof into the closures that read it, so a missed run
// point shows as a length where node throws a TypeError ("threw").

function len(f: () => number): string {
  try {
    return "len " + f();
  } catch (e) {
    return "threw";
  }
}

// Read by a hoisted function only called after the write (enterCount).
function counterAfter(k: number): number {
  let count = 0;
  function peek() {
    return (count + 1) | 0;
  }
  count = (count + k) | 0;
  count = (count * 2) | 0;
  return peek() + count;
}
console.log("counterAfter", counterAfter(1), counterAfter(5));

// Called before the write, directly and through another hoisted function.
function calledBefore(k: number): number[] {
  const early = peek();
  const viaOther = other();
  var n = k | 0;
  n = (n * 2) | 0;
  var s = "s" + k;
  s = s + "!";
  function peek() {
    return len(() => s.length) + " " + (n + 1);
  }
  function other() {
    return peek() + "+";
  }
  return [early, viaOther, peek(), other(), n];
}
console.log("calledBefore", calledBefore(3).join(" "));

// Run before the write only through another hoisted function.
function onlyThroughOther(k: number): string {
  const early = other();
  var n = k | 0;
  n = (n * 2) | 0;
  var s = "s" + k;
  s = s + "!";
  function peek() {
    return len(() => s.length) + " " + (n + 1);
  }
  function other() {
    return peek() + "+";
  }
  return early + " " + peek() + " " + n;
}
console.log("onlyThroughOther", onlyThroughOther(3));

// Passed as a callback before the write.
function callbackBefore(k: number): string {
  const out: number[] = [];
  function push() {
    out.push(len(() => s.length) + " " + (n + 1));
  }
  [1].forEach(push);
  var n = k | 0;
  n = (n + 1) | 0;
  var s = "s" + k;
  s = s + "!";
  [1].forEach(push);
  return out.join(" ") + " " + n;
}
console.log("callbackBefore", callbackBefore(4));

// Stored on an object, then called through it before the write.
function storedBefore(k: number): string {
  const o = { g: peek };
  const early = o.g();
  var n = k | 0;
  n = (n + 2) | 0;
  var s = "s" + k;
  s = s + "!";
  function peek() {
    return len(() => s.length) + " " + n * 3;
  }
  return early + " " + o.g() + " " + n;
}
console.log("storedBefore", storedBefore(2));

// Returned before the write on one path.
function returned(k: number): () => string {
  if (k < 0) {
    return peek;
  }
  var n = k | 0;
  n = (n * 2) | 0;
  var s = "s" + k;
  s = s + "!";
  function peek() {
    return len(() => s.length) + " " + (n + 1);
  }
  return peek;
}
console.log("returned", returned(-1)(), returned(3)());

// A generator called (and stepped) before the write, and after.
function generatorBefore(k: number): string {
  const early = gen().next().value;
  var n = k | 0;
  n = (n + 5) | 0;
  var s = "s" + k;
  s = s + "!";
  function* gen() {
    yield len(() => s.length) + " " + (n + 1);
  }
  return early + " " + gen().next().value + " " + n;
}
console.log("generatorBefore", generatorBefore(1));

// A recursive pair, run after the write and before it.
function pairAfter(k: number): number {
  var base = k | 0;
  base = (base + 10) | 0;
  function even(i: number): number {
    return i === 0 ? base : odd(i - 1);
  }
  function odd(i: number): number {
    return i === 0 ? base + 1 : even(i - 1);
  }
  return even(5) + base;
}
function pairBefore(k: number): string {
  const early = even(3);
  var base = k | 0;
  base = (base + 10) | 0;
  var s = "s" + k;
  s = s + "!";
  function even(i: number): string {
    return i === 0 ? len(() => s.length) + " " + base : odd(i - 1);
  }
  function odd(i: number): string {
    return i === 0 ? len(() => s.length) + " " + (base + 1) : even(i - 1);
  }
  return early + " " + even(3) + " " + base;
}
console.log("pair", pairAfter(1), pairBefore(1));

// The function name is written again: the new value may run anywhere.
function reassigned(k: number): string {
  let alias: () => number = peek;
  var n = k | 0;
  n = (n + 1) | 0;
  function peek(): number {
    return n + 1;
  }
  const before = alias();
  // @ts-ignore: a function declaration is a mutable binding
  peek = function () {
    return n + 100;
  };
  alias = peek;
  return before + " " + alias() + " " + peek() + " " + n;
}
console.log("reassigned", reassigned(2));
function reassignedEarly(k: number): string {
  // @ts-ignore
  peek = function () {
    return len(() => s.length) + " " + n * 2;
  };
  const early = peek();
  var n = k | 0;
  n = (n + 1) | 0;
  var s = "s" + k;
  s = s + "!";
  function peek(): string {
    return "";
  }
  return early + " " + peek() + " " + n;
}
console.log("reassignedEarly", reassignedEarly(2));

// A default branch that calls a function that always throws, and one whose
// "always throws" callee was replaced by one that returns.
function fail(x: number): never {
  throw new Error("illegal value " + x);
}
function neverReturns(c: number): number {
  let kind;
  switch (c) {
    case 1:
      kind = c | 0;
      break;
    case 2:
      kind = (c + 1) | 0;
      break;
    default:
      fail(c);
  }
  kind = (kind * 2) | 0;
  return kind + 1;
}
console.log("neverReturns", neverReturns(1), neverReturns(2));
try {
  neverReturns(3);
} catch (e) {
  console.log("neverReturns threw", (e as Error).message);
}
function replacedThrower(c: number): number {
  let thrower = (x: number): void => {
    throw new Error("illegal value " + x);
  };
  if (c > 5) {
    thrower = (_x: number): void => {};
  }
  let kind;
  switch (c) {
    case 1:
      kind = c | 0;
      break;
    default:
      thrower(c);
  }
  return kind + 1;
}
console.log("replacedThrower", replacedThrower(1), replacedThrower(9));
