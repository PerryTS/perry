function scan(a: any): string {
  const out: any[] = [];
  for (const x of a) out.push(x);
  return out.map(x => x === undefined ? "u" : String(x)).join(",");
}
function typedScan(a: number[]): string {
  const out: any[] = [];
  for (const x of a) out.push(x);
  return out.map(x => x === undefined ? "u" : String(x)).join(",");
}
const typedHoles: number[] = [1,2,3];
delete (typedHoles as any)[1];
console.log("typed-holes", typedScan(typedHoles));
const named: any = [1,2,3];
named.unrelated = 42;
named[Symbol.toStringTag] = "named";
console.log("unrelated-members", scan(named));
for (const kind of ["push", "pop", "zero", "splice"]) {
  const a: any = [1,2,3,4];
  let count = 0;
  const out: any[] = [];
  for (const x of a) {
    out.push(x);
    if (++count === 1) {
      if (kind === "push") a.push(5);
      if (kind === "pop") a.pop();
      if (kind === "zero") a.length = 0;
      if (kind === "splice") a.splice(1,1,9,8);
    }
  }
  console.log(kind, out.join(","));
}
const changing: any = [undefined,2,3];
const [x = (changing.length = 0,9), y = (changing.push(4,5),8), z] = changing;
console.log("completed", x,y,z);
let elisionReads = 0;
Object.defineProperty(Array.prototype, "0", {get() { elisionReads++; return 90; }, configurable: true});
const [, second] = [,2];
delete (Array.prototype as any)[0];
console.log("elision-get", second, elisionReads);
(Array.prototype as any)[1] = 42;
const holes = scan([1,,3]);
const [first, ...tail] = [1,,3];
delete (Array.prototype as any)[1];
console.log("prototype-hole", holes, first, tail.join(","));
const [, ...dense] = [1,,3];
console.log("rest-dense", 0 in dense, dense.length, dense[0], dense[1]);
let nestedA, nestedB, nestedRest;
[[nestedA, ...nestedRest], nestedB] = [[10,,12],20];
const [[boundA, ...boundRest], boundB] = [[30,,32],40];
console.log("nested", nestedA,nestedB,nestedRest[0],nestedRest[1],boundA,boundB,boundRest[0],boundRest[1]);
const [] = [];
try { const [] = 0 as any; console.log("empty-missed"); } catch { console.log("empty-throws"); }
console.log("typed-array", scan(new Uint8Array([2,4,6])));
console.log("array-like", scan({length:2, 0:5, 1:6, [Symbol.iterator]: Array.prototype.values}));
const aip: any = Object.getPrototypeOf(Array.prototype.values.call([]));
const next = aip.next;
let closeCount = 0;
for (const v of [1,2,3]) {
  aip.return = function() { closeCount++; console.log("late-return", next.call(this).value); return {done:true}; };
  break;
}
delete aip.return;
try { for (const v of [4,5,6]) {
  aip.return = function() { closeCount++; console.log("late-throw", next.call(this).value); return {done:true}; };
  throw 7;
} } catch {}
delete aip.return;
function leave(): number {
  for (const v of [7,8,9]) {
    aip.return = function() { closeCount++; console.log("late-leave", next.call(this).value); return {done:true}; };
    return v;
  }
  return 0;
}
console.log("leave", leave());
delete aip.return;
const [a = (aip.return = function() { closeCount++; console.log("late-binding", next.call(this).value); return {done:true}; }, 10)] = [undefined,11,12];
delete aip.return;
console.log("closes", closeCount,a);
const shared: any = Object.getPrototypeOf(aip);
for (const v of [13,14,15]) {
  shared.return = function() { closeCount++; console.log("shared-return", next.call(this).value); return {done:true}; };
  break;
}
delete shared.return;
for (const v of [16,17,18]) {
  (Object.prototype as any).return = function() { closeCount++; console.log("object-return", next.call(this).value); return {done:true}; };
  break;
}
delete (Object.prototype as any).return;
console.log("ancestor-closes", closeCount);
const captured: any[] = [];
for (const v of [1,2,3]) {
  captured.push(v);
  aip.next = function() { return {done:true}; };
}
aip.next = next;
console.log("captured", captured.join(","));
