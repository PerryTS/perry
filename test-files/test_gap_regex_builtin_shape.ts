// Warm each receiver before changing the fact its shape proved.
function warm(r: any) { "a a".replace(r, "x"); }
const exec = RegExp.prototype.exec;
let own: any = /a/g;
warm(own);
let calls = 0;
own.exec = function () { calls++; return null; };
console.log("own exec", "a a".replace(own, "x"), calls);

const flags = Object.getOwnPropertyDescriptor(RegExp.prototype, "flags")!;
warm(/a/g);
let order: string[] = [];
Object.defineProperty(RegExp.prototype, "flags", { configurable: true,
  get() { order.push("flags"); return ""; } });
const patched: any = /a/g;
patched.exec = function (s: string) { order.push("exec"); return exec.call(this, s); };
console.log("proto flags", "a a".replace(patched, "x"), order.join(","));
Object.defineProperty(RegExp.prototype, "flags", flags);

const replace = RegExp.prototype[Symbol.replace];
warm(/a/g);
RegExp.prototype[Symbol.replace] = function (s: string, _replacement: any) { return "patched:" + s; };
console.log("proto replace", "a a".replace(/a/g, "x"));
RegExp.prototype[Symbol.replace] = replace;

for (const name of ["hasIndices", "global", "ignoreCase", "multiline", "dotAll", "unicode", "unicodeSets", "sticky"]) {
  const desc = Object.getOwnPropertyDescriptor(RegExp.prototype, name)!;
  const r: any = /a/g;
  warm(r);
  let seen = 0;
  Object.defineProperty(r, name, { configurable: true, get() { seen++; return false; } });
  console.log("own flag", name, "a a".replace(r, "x"), seen);
  const next: any = /a/g;
  warm(next);
  let got = 0;
  Object.defineProperty(RegExp.prototype, name, { configurable: true, get() { got++; return false; } });
  console.log("proto flag", name, "a a".replace(next, "x"), got);
  Object.defineProperty(RegExp.prototype, name, desc);
}
// Full observable flag Get order survives aggregate proof refusal.
const ordered: any = /a/g;
order = [];
for (const name of ["hasIndices", "global", "ignoreCase", "multiline", "dotAll", "unicode", "unicodeSets", "sticky"]) {
  Object.defineProperty(ordered, name, { get() { order.push(name); return name === "global"; } });
}
"a a".replace(ordered, "x");
// Node 26.5.1 reads sticky before unicodeSets; ECMA-262 requires v before y.
// The runtime unit test checks the complete spec order. Compare the shared
// prefix and both remaining Get counts here.
console.log("order", order.slice(0, 6).join(","),
  order.filter(name => name === "unicodeSets").length,
  order.filter(name => name === "sticky").length);

const last: any = /a/;
warm(last);
let coerces = 0;
last.lastIndex = { valueOf() { coerces++; return 0; } };
console.log("lastIndex", "a".replace(last, "x"), coerces);

for (const symbol of [Symbol.replace, Symbol.split, Symbol.match, Symbol.search, Symbol.matchAll]) {
  const r: any = /a/g;
  warm(r);
  r[symbol] = function () { return "own-symbol"; };
  console.log("symbol", symbol.description, typeof r[symbol].call(r, "a", "x"));
}
const split: any = / /;
"a a".split(split);
split[Symbol.split] = function () { return ["patched"]; };
console.log("split", "a a".split(split).join("|"));
const match: any = /a/;
"a".match(match);
match[Symbol.match] = function () { return ["patched"]; };
console.log("match", "a".match(match)!.join("|"));
const search: any = /a/;
"a".search(search);
search[Symbol.search] = function () { return 42; };
console.log("search", "a".search(search));
const custom: any = /a/g;
warm(custom);
Object.setPrototypeOf(custom, { [Symbol.replace]() { return "custom-prototype"; } });
console.log("prototype", "a".replace(custom, "x"));

// The aggregate proof is also shared by builtin test/exec admission.
const protoExec = RegExp.prototype.exec;
const execReceiver = /a/;
console.log("warm test", execReceiver.test("a"));
let protoExecCalls = 0;
RegExp.prototype.exec = function (_s: string) { protoExecCalls++; return null; };
console.log("proto exec test", execReceiver.test("a"), protoExecCalls);
console.log("proto exec replace", "a".replace(/a/g, "x"), protoExecCalls);
RegExp.prototype.exec = protoExec;
