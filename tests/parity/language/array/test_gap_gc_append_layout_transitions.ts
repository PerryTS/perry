// The append guard reads the live receiver after the RHS. Layout changes,
// string sharing and the element store precede publication of the new length.
function append(a: any, v: any): any {
  a[a.length] = v;
  return a;
}
function show(label: string, a: any): void {
  console.log(label, a.length, JSON.stringify(a));
}
const direct: any[] = [];
direct[direct.length] = { value: 7 };
direct[direct.length] = 2.5;
direct[direct.length] = "short";
const mixed: any[] = [];
for (const v of [0, 1.25, "short", { x: 3 }, null, undefined, true, 4]) append(mixed, v);
show("mixed", mixed);
const pointers: any[] = [];
append(pointers, { x: 1 });
append(pointers, { x: 2 });
append(pointers, 3.25);
append(pointers, { x: 4 });
show("pointer transition", pointers);
const holey = new Array(3);
append(holey, { x: 1 });
append(holey, 2);
show("holes", holey);
console.log("hole presence", 0 in holey, 2 in holey, 3 in holey);
const named: any = [1];
named.label = "keep";
append(named, "next");
show("named", named);
console.log(named.label);
const sparse: any[] = [];
sparse[100000] = "far";
sparse.length = 2;
append(sparse, "dense");
show("sparse to dense", sparse);
for (const kind of ["freeze", "seal", "preventExtensions", "length"]) {
  const a: any = [1];
  if (kind === "freeze") Object.freeze(a);
  if (kind === "seal") Object.seal(a);
  if (kind === "preventExtensions") Object.preventExtensions(a);
  if (kind === "length") Object.defineProperty(a, "length", { writable: false });
  try {
    (function (receiver: any) { "use strict"; receiver[receiver.length] = 2; })(a);
    console.log(kind, "no throw");
  } catch (e: any) { console.log(kind, e instanceof TypeError); }
  show(kind, a);
}
let intercepted = "";
const custom: any[] = [];
const proto = Object.create(Array.prototype);
Object.defineProperty(proto, "0", { set(v) { intercepted += v; } });
Object.setPrototypeOf(custom, proto);
append(custom, "setter");
console.log("inherited", custom.length, intercepted);
class Items extends Array<any> {}
const sub = new Items();
append(sub, { x: 5 });
sub.push(6);
show("subclass", sub);
console.log(sub instanceof Items, sub instanceof Array);
let len = 0;
const arraylike: any = {
  get length() { return len; },
  set length(v: number) { console.log("length setter", v); len = v; }
};
append(arraylike, "index");
console.log("arraylike", arraylike[0], len);
Array.prototype.push.call(arraylike, "push");
console.log("arraylike push", arraylike[0], len);
let a: any[] = ["first"];
a[a.length] = (a.length = 0, "rhs");
show("rhs length", a);
const original: any[] = [];
let rebound: any[] = original;
rebound[rebound.length] = (rebound = ["replacement"], "original");
show("evaluated receiver", original);
show("binding", rebound);
let s = "this is a long unique string";
for (let i = 0; i < 12; i++) s += "!";
const saved: string[] = [];
append(saved, s);
s += "tail";
console.log("shared", saved[0].length, s.length, saved[0].endsWith("!"));

// A bulk append can request growth while capacity still exceeds used length.
const partial: any[] = [];
for (let i = 0; i < 32; i++) partial.push({ i });
partial.length = 2;
const alias = partial;
partial.push(...Array.from({ length: 40 }, (_, i) => i + 100));
console.log("partial grow", partial.length, alias.length, partial[0].i, partial[1].i, partial[2], partial[41]);
// Collector-visible transitions with an existing scalar prefix.
const retained: any[] = [];
append(retained, 0);
for (let i = 0; i < 64; i++) append(retained, { i, text: "retained " + i });
let total = 0;
for (let i = 1; i < retained.length; i++) total += retained[i].i;
console.log("retained", retained.length, total, retained[64].text);

console.log("direct after GC", direct[0].value, direct[1], direct[2]);
