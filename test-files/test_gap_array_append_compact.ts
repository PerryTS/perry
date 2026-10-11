"use strict";
function numbers(a: any) { a[a.length] = 1.25; }
function objects(a: any, v: any) { a[a.length] = {v}; }
function strings(a: any, v: any) { a[a.length] = v; }
const a: any = [];
for (let i = 0; i < 80; i++) numbers(a);
objects(a, 12); strings(a, "long alias string"); numbers(a);
console.log("mixed", a.length, a[0], a[80].v, a[81], a[82]);
const holes: any = new Array(3); numbers(holes);
console.log("holes", holes.length, 0 in holes, 3 in holes, holes[3]);
const constrained: any = Object.freeze([1]);
try { numbers(constrained); } catch (e) { console.log("frozen", e instanceof TypeError); }
const bytes: any = new Uint8Array(2); numbers(bytes);
console.log("bytes", bytes.length, bytes[2]);
const fake: any = {length: 0}; numbers(fake);
console.log("object", fake.length, fake[0]);
// A canonical numeric result may still run user coercion before the store.
const changed: any = [];
changed[changed.length] = Number({valueOf() { changed.push(7); return 9; }});
console.log("numeric RHS", changed.length, changed[0]);
const grown: any = []; const alias: any = grown;
grown[grown.length] = Number({valueOf() { for (let i = 0; i < 40; i++) grown.push(i); return 12; }});
console.log("numeric grow RHS", alias.length, alias[0], alias[39]);
let calls = 0;
const p: any = [];
Object.setPrototypeOf(p, {set 0(v) { calls++; }});
numbers(p); objects(p, 2);
console.log("custom", calls, p.length, Object.hasOwn(p, "0"));
let seen = 0;
Object.defineProperty(Array.prototype, "0", {set(v) { seen = v; }, configurable: true});
const clean: any = []; numbers(clean);
const output = [seen, clean.length, Object.hasOwn(clean, "0")];
delete (Array.prototype as any)[0];
console.log("inherited", output[0], output[1], output[2]);
const proto: any = Array.prototype; numbers(proto);
const inherited: any = [];
console.log("prototype receiver", inherited[0], 0 in inherited);
delete proto[0]; proto.length = 0;
