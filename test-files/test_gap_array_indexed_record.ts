function scan(source: any): string {
  const values: any[] = [];
  for (const x of source) values.push(x);
  return values.map(x => x === undefined ? "u" : String(x)).join(",");
}
function typedScan(source: number[]): string {
  const values: any[] = [];
  for (const x of source) values.push(x);
  return values.map(x => x === undefined ? "u" : String(x)).join(",");
}
console.log("plain", scan([1,2,3,4]), typedScan([1,2,3,4]));
const sparse: any[] = [1,,3,4];
console.log("holes", scan(sparse), typedScan(sparse));
const values = Array.prototype[Symbol.iterator];
const aip: any = Object.getPrototypeOf(values.call([]));
const next = aip.next;
console.log("exposure", scan([1,2,3]));
const own: any = [1,2,3];
own[Symbol.iterator] = function() {
  let i = 0;
  return { next() { return {value: 70 + i++, done: i > 2}; } };
};
console.log("own", scan(own), typedScan(own));
const inherited: any = [1,2,3];
Object.setPrototypeOf(inherited, { [Symbol.iterator]: own[Symbol.iterator] });
console.log("inherited", scan(inherited), typedScan(inherited));
Array.prototype[Symbol.iterator] = own[Symbol.iterator];
console.log("prototype", scan([1,2,3]), typedScan([1,2,3]));
Array.prototype[Symbol.iterator] = values;
aip.next = function() { const r = next.call(this); if (!r.done) r.value += 100; return r; };
console.log("next", scan([1,2,3]), typedScan([1,2,3]));
aip.next = next;
for (const kind of ["push", "pop", "zero", "splice"]) {
  const a: any = [1,2,3,4];
  const out: any[] = [];
  for (const v of a) {
    out.push(v);
    if (out.length === 1) {
      if (kind === "push") a.push(5);
      if (kind === "pop") a.pop();
      if (kind === "zero") a.length = 0;
      if (kind === "splice") a.splice(1, 1, 9, 8);
    }
  }
  console.log("mutation", kind, out.join(","));
}
const out: any[] = [];
for (const v of [1,2,3]) {
  out.push(v);
  aip.next = function() { return {done: true}; };
}
console.log("captured-next", out.join(","));
aip.next = next;
(Array.prototype as any)[1] = 42;
const holeResult = scan([1,,3]);
delete (Array.prototype as any)[1];
console.log("prototype-hole", holeResult);
const d: any = [1,2,3];
const [a,b,c] = d;
console.log("destructure", a,b,c);
const [o,p,q] = own;
console.log("destructure-own", o,p,q);
const changing: any = [undefined, 2, 3];
const [x = (changing.length = 0, 9), y = (changing.push(4,5), 8), z] = changing;
console.log("destructure-completed", x,y,z);
const elision: any[] = [1,2,3];
let getterReads = 0;
Object.defineProperty(elision, "0", {get() { getterReads++; return 1; }, configurable: true});
const [, e] = elision;
console.log("elision", e, getterReads);
let inheritedGetterReads = 0;
Object.defineProperty(Array.prototype, "0", {get() { inheritedGetterReads++; return 90; }, configurable: true});
const [, inheritedElision] = [,2];
delete (Array.prototype as any)[0];
console.log("elision-inherited-get", inheritedElision, inheritedGetterReads);
let closes = 0;
function custom(): any {
  return { [Symbol.iterator]() { let n = 0; return {
    next() { return {value: ++n, done: n > 3}; },
    return() { closes++; return {done:true}; }
  }; } };
}
for (const v of custom()) break;
try { for (const v of custom()) throw 1; } catch {}
function leave(): any { for (const v of custom()) return v; }
leave();
const [first] = custom();
console.log("close-custom", closes, first);
let builtinCloses = 0;
aip.return = function() { builtinCloses++; console.log("close-cursor", next.call(this).value); return {done:true}; };
for (const v of [1,2,3]) break;
try { for (const v of [4,5,6]) throw 1; } catch {}
function builtinLeave(): any { for (const v of [7,8,9]) return v; }
builtinLeave();
const [last] = [10,11,12];
delete aip.return;
console.log("close-builtin", builtinCloses, last);
const grow: any = [1,2,3];
const alias: any = grow;
for (let i=0;i<100;i++) grow.push(i);
grow[Symbol.iterator] = own[Symbol.iterator];
console.log("growth-alias", scan(alias), typedScan(alias));
let nextGets = 0;
Object.defineProperty(aip, "next", {get() { nextGets++; return next; }, configurable: true});
console.log("next-accessor", scan([1,2,3]), nextGets);
delete aip.next;
aip.next = next;
let protoGets = 0;
Object.defineProperty(Array.prototype, Symbol.iterator, {get() { protoGets++; return own[Symbol.iterator]; }, configurable: true});
console.log("prototype-accessor", scan([1,2,3]), protoGets);
Object.defineProperty(Array.prototype, Symbol.iterator, {value: values, writable: true, configurable: true});
