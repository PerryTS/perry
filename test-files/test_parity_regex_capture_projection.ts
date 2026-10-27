// Capture-demand projection: complete-answer and call-order witnesses.
function capture(re: any, input: any, n: number): void {
  // Dynamic index deliberately keeps the complete exec result.
  const m = re.exec(input);
  console.log(m === null ? null : m[n]);
}
function one(re: any, input: any): void {
  const m = re.exec(input);
  if (m !== null) console.log(m[1]);
  else console.log("no match");
}
one(/(a)?b/, "b");
one(/(a|ab)+b/, "aabb");
one(/(😀)/u, "😀");
function surrogate(): void {
  const re = /(.)/;
  const m = re.exec("😀");
  if (m !== null) console.log(m[1].charCodeAt(0));
}
surrogate();
one(/()/g, "x");
one(/(x)/, "no");
one(/(?<name>x)/d, "x");
const state = /(a)/gy;
one(state, "aa"); console.log(state.lastIndex);
one(state, "aa"); console.log(state.lastIndex);
one(state, "aa"); console.log(state.lastIndex);
state.lastIndex = -1; one(state, "aa"); console.log(state.lastIndex);
const coerce = /(x)/;
coerce.lastIndex = { valueOf() { console.log("lastIndex"); return 1; } } as any;
one(coerce, "x");
let changed: any = /(x)/;
let gets = 0;
const object: any = {};
Object.defineProperty(object, "exec", { get() { gets++; console.log("get", gets); return function(input: string) { console.log(this === object, input); return ["all", "override"]; }; } });
one(object, "argument");
const throwing: any = { exec: 4 };
try { one(throwing, "x"); } catch (e) { console.log(e instanceof TypeError); }
const overridden: any = /(x)/;
overridden.exec = function(s: string) { console.log("override", this === overridden, s); return ["", "99"]; };
one(overridden, "x");
function ordering(): void {
  const re: any = /(x)/;
  const method = re.exec;
  let reads = 0;
  Object.defineProperty(re, "exec", { get() { reads++; console.log("get before argument"); return method; }, configurable: true });
  const input: any = { get value() { console.log("argument"); Object.defineProperty(re, "exec", { value: function() { return ["", "wrong"]; }, configurable: true }); return "x"; } };
  const m = re.exec(input.value);
  if (m !== null) console.log(m[1]);
  console.log(reads);
}
ordering();
function receiverOrder(): void {
  let re: any = /(x)/;
  const old = re;
  const method = re.exec;
  Object.defineProperty(re, "exec", { get() { re = {}; return method; } });
  const m = re.exec("x");
  if (m !== null) console.log(m[1]);
  console.log(re === old);
}
receiverOrder();
one(/(x)/, { toString() { console.log("coerce"); return "x"; } });
try { one(/(x)/, Symbol("x")); } catch (e) { console.log(e instanceof TypeError); }
const frozen = /(x)/g;
Object.defineProperty(frozen, "lastIndex", { writable: false });
try { one(frozen, "x"); } catch (e) { console.log(e instanceof TypeError); }
// Metadata/escaping/multiple captures must keep full construction.
const full = /(?<a>x)(y)?/d.exec("x")!;
console.log(full[0], full[1], full[2], full.index, full.input, full.groups!.a);
console.log(JSON.stringify(full.indices));
for (const text of ["0012", "-0", "  +12rest", "0x10", "Infinity", "9007199254740991", "", "-15"]) {
  const re = /(.*)/;
  const m = re.exec(text);
  if (m !== null) console.log(parseInt(m[1], 10), Object.is(parseInt(m[1], 10), -0));
}
capture(/(a)(b)/, "ab", 2);
function optionalOwn(): void {
  const re = /(a)?b/;
  const m = re.exec("b");
  (Array.prototype as any)[1] = "inherited";
  if (m !== null) console.log(m[1]);
  delete (Array.prototype as any)[1];
}
optionalOwn();
const symbolPrimitive: any = { [Symbol.toPrimitive]() { return Symbol("x"); } };
try { one(/(x)/, symbolPrimitive); } catch (e) { console.log(e instanceof TypeError); }
const bound: any = /(a)/;
bound.exec = /(b)/.exec.bind(/(b)/);
one(bound, "b");
function inheritedOutOfRange(): void {
  Object.defineProperty(Array.prototype, "3", { configurable: true, get() { return this[0] + ":" + this.index + ":" + this.input; } });
  const re = /(x)/;
  const m = re.exec("x");
  if (m !== null) console.log(m[3]);
  delete (Array.prototype as any)[3];
}
inheritedOutOfRange();
function deletedCapture(): void {
  Object.defineProperty(Array.prototype, "1", { configurable: true, get() { return this[0] + ":" + this.index + ":" + this.input; } });
  const re = /(x)/;
  const m = re.exec("x");
  if (m !== null) { delete (m as any)[1]; console.log(m[1]); }
  delete (Array.prototype as any)[1];
}
deletedCapture();
