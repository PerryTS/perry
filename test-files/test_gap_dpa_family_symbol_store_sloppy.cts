// DPA-F1 #2 (sloppy): writable:false is ignored, a setter runs.
var s = Symbol("s");
function F() { this[s] = 1; }
var w = {};
Object.defineProperty(w, s, { value: 5, writable: false, enumerable: true, configurable: true });
try { F.call(w); console.log("sloppy no throw", w[s]); } catch (e) { console.log("sloppy threw", e instanceof TypeError, w[s]); }
var o = {};
Object.defineProperty(o, s, { get: function () { return "g"; }, set: function (v) { console.log("sloppy setter ran", v); }, configurable: true });
F.call(o);
console.log("sloppy accessor after", o[s]);
