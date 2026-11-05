"use strict";

// Keep the small functions behind runtime dispatch so inlining cannot hide
// their alias bookkeeping or the closure-typed parameter call under test.
function identity(x: any): any { return x; }
function aliasOne(x: any): any { const y = x; return y; }
function aliasThree(x: any): any { const a = x; const b = a; const c = b; return c; }
function deadAssignments(x: any): any { let y = x; y = x; y = x; return x; }
function changedSource(x: any): any { const y = x; x = "changed"; return y; }
function changedAlias(x: any): any { let y = x; y = "changed"; return y; }
function capturedAlias(x: any): any { const y = x; return () => y; }
function appendedSource(x: any): any { const y = x; x += "-appended"; return y; }
function aliasAcrossCall(x: any, visit: () => void): any { const y = x; visit(); return y; }
const copies: any[] = [identity, aliasOne, aliasThree, deadAssignments, changedSource];
const values: any[] = [0, -0, NaN, 17, true, null, undefined,
  "a string longer than the inline string representation", 12345678901234567890n,
  Symbol("value"), { value: 17 }, [1, 2, 3]];
let copyFailures = 0;
for (let i = 0; i < copies.length; i++) {
  for (let j = 0; j < values.length; j++) {
    if (!Object.is(copies[i](values[j]), values[j])) copyFailures++;
  }
}
console.log("copies", copyFailures, changedAlias("original"), capturedAlias(19)());
console.log("string-copy", appendedSource("a heap string which must remain unchanged"));

let effects = 0;
function effect(): any { effects++; return { effects }; }
function deadEffect(x: any): any { let y = x; y = effect(); return x; }
console.log("effects", deadEffect(7), effects);
function tdz(): void {
  try { console.log(later); const later = 1; }
  catch (e) { console.log("tdz", e instanceof ReferenceError); }
}
tdz();
// Sloppy mapped arguments can change the source after its alias was copied.
const mapped = Function("x", "const y = x; arguments[0] = 99; return y;");
console.log("mapped", mapped(7));

let sink: any = null;
function storeScalar(): any { sink = 3; return sink; }
function storeUnknown(value: any): any { sink = value; return sink; }
function storeDeclaredNumber(value: number): any { sink = value; return sink; }
const stores: any[] = [storeUnknown, storeDeclaredNumber];
console.log("scalar", storeScalar());
let storeFailures = 0;
for (let i = 0; i < stores.length; i++) {
  for (let j = 0; j < values.length; j++) {
    if (!Object.is(stores[i](values[j]), values[j])) storeFailures++;
  }
}
console.log("stores", storeFailures);

function invoke(callback: (x: any) => any, x: any): any { return callback(x); }
const dispatchers: any[] = [invoke];
function churn(): number {
  const keep: any[] = [];
  for (let i = 0; i < 80; i++) keep.push({ i, text: "allocation-" + i, pad: [i] });
  return keep.length;
}
const strictCallback = function (this: any, x: any): any {
  "use strict";
  churn();
  return [this === undefined, x];
};
const host: any = {
  marker: "host",
  run: function (this: any, callback: (x: any) => any): any {
    const result = dispatchers[0](callback, 23);
    return [result, this.marker];
  },
};
console.log("strict", JSON.stringify(host.run(strictCallback)));
const lexical: any = {
  marker: "lexical",
  make: function (this: any): any { return (x: any) => { churn(); return this.marker + x; }; },
};
console.log("arrow", JSON.stringify(host.run(lexical.make())));
const bound = function (this: any, x: any): any { churn(); return this.marker + x; }
  .bind({ marker: "bound" });
console.log("bound", JSON.stringify(host.run(bound)));
const rest = function (this: any, ...xs: any[]): any {
  return [this === undefined, xs.length, xs[0]];
};
console.log("rest", JSON.stringify(host.run(rest)));
const proxy = new Proxy(strictCallback, {
  apply: function (target: any, receiver: any, args: any[]): any {
    return [receiver === undefined, Reflect.apply(target, receiver, args)];
  },
});
console.log("proxy", JSON.stringify(host.run(proxy)));
try { host.run(function (x: any): any { churn(); throw new Error("callback-" + x); }); }
catch (e: any) { console.log("throw", e.message); }
console.log("after-throw", JSON.stringify(host.run(strictCallback)));
let stressFailures = 0;
const acrossCalls: any[] = [aliasAcrossCall];
for (let i = 0; i < 40; i++) {
  const item: any = { marker: "fresh-" + i, run: host.run };
  const kept = acrossCalls[0](item, churn);
  if (kept !== item || kept.marker !== "fresh-" + i) stressFailures++;
  const result = item.run(strictCallback);
  if (result[0][0] !== true || result[0][1] !== 23 || result[1] !== item.marker) stressFailures++;
}
console.log("stress", stressFailures);
