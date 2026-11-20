"use strict";
function f() { return arguments; }
function probe(a) { try { a.callee; return "no-throw"; } catch (e) { return e instanceof TypeError ? "TypeError" : "other"; } }
const a0 = f(1);
const d0 = Object.getOwnPropertyDescriptor(a0, "callee");
console.log(probe(a0), typeof d0.get, d0.get === d0.set);
let keep = [];
for (let r = 0; r < 30; r++) { let junk = []; for (let i = 0; i < 20000; i++) junk.push({ i: i, s: "x" + i }); keep.push(junk[r]); const g = (globalThis as any).gc; if (typeof g === "function") g(); }
const a1 = f(2, 3);
const d1 = Object.getOwnPropertyDescriptor(a1, "callee");
console.log(probe(a1), probe(a0), d1.get === d0.get, d1.set === d0.get, keep.length);
try { d1.get(); console.log("no-throw"); } catch (e) { console.log(e instanceof TypeError); }
