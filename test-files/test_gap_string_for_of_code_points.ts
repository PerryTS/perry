// #10062: for-of yields code points; indexing and charCodeAt yield code units.
function equal(actual: string, expected: string): void {
  if (actual !== expected) throw new Error(actual + " !== " + expected);
}

// Include EVERY code unit, so an astral element with a missing/wrong low
// surrogate cannot satisfy the check merely by reporting length two.
function describe(ch: string): string {
  let out = "" + ch.length;
  for (let i = 0; i < ch.length; i++) out += ":" + ch.charCodeAt(i);
  return out + ",";
}

function typed(s: string): string {
  let out = "";
  for (const ch of s) out += describe(ch);
  return out;
}

function dynamic(s: any): string {
  let out = "";
  for (const ch of s) out += describe(ch);
  return out;
}

const mixed = "\u00e4\u4e2d\u{1f600}\u00d6";
const mixedExpected = "1:228,1:20013,2:55357:56832,1:214,";
equal(typed(mixed), mixedExpected);
equal(dynamic(mixed), mixedExpected);
equal(typed(""), "");
equal(dynamic(""), "");
equal(typed("aBcD"), "1:97,1:66,1:99,1:68,");
equal(dynamic("aBcD"), "1:97,1:66,1:99,1:68,");

const adjacent = "\u{1f600}\u{1f680}\u{10000}\u{10ffff}";
const adjacentExpected = "2:55357:56832,2:55357:56960,2:55296:56320,2:56319:57343,";
equal(typed(adjacent), adjacentExpected);
equal(dynamic(adjacent), adjacentExpected);

// Lone surrogates stay individual values, including a low/high sequence.
const lone = "\ud800A\udc00\udfff\udbffZ";
const loneExpected = "1:55296,1:65,1:56320,1:57343,1:56319,1:90,";
equal(typed(lone), loneExpected);
equal(dynamic(lone), loneExpected);
equal(typed(String.fromCharCode(0xd83d, 0xde00)), "2:55357:56832,");

function local(): string {
  let source: string = "\u00e4\u4e2d\u{1f600}\u00d6";
  let out = "";
  for (const ch of source) out += describe(ch);
  return out;
}
equal(local(), mixedExpected);

// Module initialization has a separate for-of lowering implementation.
let top = "";
for (const ch of mixed) top += describe(ch);
equal(top, mixedExpected);
let literal = "";
for (const ch of "\u{1f600}\u{1f680}") literal += describe(ch);
equal(literal, "2:55357:56832,2:55357:56960,");
const topDynamic: any = mixed;
let genericTop = "";
for (const ch of topDynamic) genericTop += describe(ch);
equal(genericTop, mixedExpected);

// Continue must advance exactly once; break must not process the suffix.
function control(s: string): string {
  let out = "";
  for (const ch of s) {
    if (ch === "\u{1f600}") continue;
    if (ch === "!") break;
    out += describe(ch);
  }
  return out;
}
equal(control("\u{1f600}A\u{1f600}\u{1f680}!Z"), "1:65,2:55357:56960,");
let topControl = "";
for (const ch of "\u{1f600}A\u{1f600}\u{1f680}!Z") {
  if (ch === "\u{1f600}") continue;
  if (ch === "!") break;
  topControl += describe(ch);
}
equal(topControl, "1:65,2:55357:56960,");

function first(s: string): string {
  for (const ch of s) return ch;
  return "";
}
equal(describe(first(adjacent)), "2:55357:56832,");
equal(first(""), "");

let assigned = "";
let assignmentItems = "";
for (assigned of adjacent) assignmentItems += describe(assigned);
equal(assignmentItems, adjacentExpected);

// Indexed access still exposes BOTH halves separately.
function indexed(s: string): string {
  return describe(s[0]) + describe(s[1]) + s.charCodeAt(0) + ":" + s.charCodeAt(1);
}
equal(indexed("\u{1f600}"), "1:55357,1:56832,55357:56832");

// A guarded array loop also lowers its nested string loop in forced lazy
// mode. Its holder must remain an iterator object in that alternative arm.
function nested(inputs: string[]): string {
  let out = "";
  for (const input of inputs) {
    for (const ch of input) out += describe(ch);
  }
  return out;
}
equal(nested([mixed]), mixedExpected);
const originalArrayIterator = Array.prototype[Symbol.iterator];
Array.prototype[Symbol.iterator] = function () {
  return originalArrayIterator.call(this);
};
equal(nested([adjacent]), adjacentExpected);
Array.prototype[Symbol.iterator] = originalArrayIterator;

console.log("string for-of code points: ok");

async function asyncTyped(s: string): Promise<string> {
  let out = "";
  for await (const ch of s) {
    if (ch === "\u00e4") continue;
    out += describe(ch);
  }
  return out;
}
asyncTyped(mixed).then((out: string) => {
  equal(out, "1:20013,2:55357:56832,1:214,");
  console.log("async string for-of code points: ok");
});
