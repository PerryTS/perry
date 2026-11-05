// A generator that throws mid-spread: no call happens, the error propagates, earlier args were evaluated.
let calls = 0, log = [];
class V { set(a, b, c, d) { calls++; return a + b + c + d; } }
class G { constructor() { this.x = 1; } *[Symbol.iterator]() { yield this.x; log.push("before"); throw new Error("boom"); } }
const v = new V();
try { v.set((log.push("arg0"), 0), ...new G(), (log.push("arg2"), 2)); } catch (e) { console.log("caught", e.message); }
console.log(calls, log.join(","));
function f(a, b) { calls++; return a + b; }
try { f(...new G()); } catch (e) { console.log("caught", e.message); }
console.log(calls);
