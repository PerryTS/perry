// #12658: a hoisted `var` read before its first write holds `undefined`, not
// a Number. A local typed Number by construction must have no read that can
// run before a write; each case below reads the binding while it is still
// the hoisted `undefined` seed (or, for the control, never does).

// The issue case: the read sits in a branch above the declaration.
function issue(k: number): number {
  var r = 0;
  if (k) {
    r = n + 1;
  }
  var n = 5;
  n = n * 2;
  return r + n;
}
console.log("issue", issue(1), issue(0));

// Loop-carried: the first iteration reads the binding before any write.
function loopCarried(m: number): number {
  var s = 0;
  for (var i = 0; i < m; i++) {
    s = s + t;
    var t = i * 2;
    t = t + 1;
  }
  return s;
}
console.log("loopCarried", loopCarried(3), loopCarried(0));

// The read inside a later iteration only (previous iteration wrote it):
// a Number in node, but the first-iteration path is skipped by the guard.
function loopGuarded(m: number): number {
  var s = 0;
  for (var i = 0; i < m; i++) {
    if (i > 0) s = s + u;
    var u = i * 2;
    u = u + 0.5;
  }
  return s;
}
console.log("loopGuarded", loopGuarded(4));

// A closure called before the declaration reads the seed.
function closureBefore(): number {
  function get(): number {
    return q + 1;
  }
  var r = get();
  var q = 10;
  q = q * 3;
  return r + q;
}
console.log("closureBefore", closureBefore());

// Same, with the captured binding written only once.
function closureBeforeConst(): number {
  function get(): number {
    return w * 2;
  }
  var r = get();
  var w = 10;
  return r + w;
}
console.log("closureBeforeConst", closureBeforeConst());

// An arrow run immediately (callback) before the declaration.
function arrowBefore(): string {
  var seen = [1, 2].map(() => v + 1).join(",");
  var v = 3;
  v = v * v;
  return seen + "|" + v;
}
console.log("arrowBefore", arrowBefore());

// typeof of the seed.
function typeofBefore(): string {
  var t = typeof n2;
  var n2 = 1;
  n2 = n2 + 1;
  return t + ":" + n2;
}
console.log("typeofBefore", typeofBefore());

// `+` with a string: undefined concatenates as "undefined".
function stringPlus(): string {
  var out = "x" + n3;
  var n3 = 3;
  n3 = n3 * 2;
  return out + ":" + n3;
}
console.log("stringPlus", stringPlus());

// Strict equality with undefined sees the seed.
function eqUndefined(): string {
  var e = n4 === undefined;
  var n4 = 2;
  n4 = n4 * n4;
  return e + ":" + n4;
}
console.log("eqUndefined", eqUndefined());

// The seed flows through a copy into another local.
function aliasCopy(k: number): string {
  var r = 0;
  if (k) r = n5;
  var n5 = 5;
  n5 = n5 * 2;
  return String(r) + ":" + typeof r + ":" + n5;
}
console.log("aliasCopy", aliasCopy(1), aliasCopy(0));

// Write in only one branch: the other path reads the seed.
function oneBranch(c: number): number {
  if (c) {
    var n6 = 4;
    n6 = n6 * 2;
  }
  return n6 + 1;
}
console.log("oneBranch", oneBranch(1), oneBranch(0));

// Increment of the seed: ToNumeric(undefined) is NaN.
function updateBefore(): string {
  var a = 0;
  a = n7++;
  var n7 = 1;
  n7 = n7 + 1;
  return a + ":" + n7;
}
console.log("updateBefore", updateBefore());

// The int32 shapes: the binding would otherwise be an int32 local.
function bitOr(k: number, x: number): number {
  var r = 7;
  if (k) {
    r = n8 + 1;
  }
  var n8 = x | 0;
  n8 = (n8 + 1) | 0;
  return r;
}
console.log("bitOr", bitOr(1, 9), bitOr(0, 9));

function unsignedShift(k: number, x: number): number {
  var r = 7;
  if (k) {
    r = n9 + 1;
  }
  var n9 = x >>> 0;
  n9 = n9 >>> 1;
  return r;
}
console.log("unsignedShift", unsignedShift(1, 9));

// The read reaches the seed through a switch case, a catch block, a
// do-while body and a conditional expression.
function viaSwitch(k: number): number {
  var r = 0;
  switch (k) {
    case 1:
      r = s1 + 1;
      break;
    case 2:
      var s1 = 3;
      s1 = s1 + 1;
      r = s1;
  }
  return r;
}
console.log("viaSwitch", viaSwitch(1), viaSwitch(2));

function viaCatch(k: number): number {
  var r = 0;
  try {
    if (k) throw 1;
    var c1 = 3;
    c1 = c1 + 1;
  } catch (e) {
    r = c1 + 1;
  }
  return r;
}
console.log("viaCatch", viaCatch(1), viaCatch(0));

function viaDoWhile(m: number): number {
  var r = 0;
  do {
    r = r + d1;
    var d1 = 2;
    d1 = d1 + 1;
  } while (--m > 0);
  return r;
}
console.log("viaDoWhile", viaDoWhile(2));

function viaConditional(k: number): number {
  var r = k ? e1 + 1 : 0;
  var e1 = 3;
  e1 = e1 + 1;
  return r;
}
console.log("viaConditional", viaConditional(1), viaConditional(0));

// String concatenation of an int-shaped seed.
function intToString(k: number): string {
  var r = "a";
  if (k) {
    r = "" + f1;
  }
  var f1 = 5;
  f1 = f1 + 1;
  return r + ":" + f1;
}
console.log("intToString", intToString(1));

// Control: declared then used, every read after a Number write. Must stay
// correct (and typed: the codegen unit test checks the typing).
function control(m: number): number {
  var x = 1.5;
  for (var i = 0; i < m; i++) {
    x = x * 2 + i;
  }
  return x;
}
console.log("control", control(5));
