// #9810: run with Node or compile with Perry. Arguments: receiver length, calls.
// .cjs keeps the receiver-binding controls in sloppy mode on both engines.
const n = Number(process.argv[3] || "20000");
const receiver = "x".repeat(Number(process.argv[2] || "200"));
function unused(value) { return value + 1; }
function observed(value) { return this.length + value; }
function capture() { return this; }
function strict(value) { "use strict"; return value + 1; }
String.prototype.bench9810 = unused;
const funcs = { unused, observed, strict, capture };
let sum = 0;
let start = Date.now();
for (let i = 0; i < n; i++) sum += receiver.bench9810(i);
console.log("method", receiver.length, Date.now() - start, sum);
sum = 0; start = Date.now();
for (let i = 0; i < n; i++) sum += funcs.unused.call(receiver, i);
console.log("call", receiver.length, Date.now() - start, sum);
sum = 0; start = Date.now();
for (let i = 0; i < n; i++) sum += funcs.observed.call(receiver, i);
console.log("call-this", receiver.length, Date.now() - start, sum);
sum = 0; start = Date.now();
for (let i = 0; i < n; i++) sum += funcs.unused.apply(receiver, [i]);
console.log("apply", receiver.length, Date.now() - start, sum);
sum = 0; start = Date.now();
for (let i = 0; i < n; i++) sum += funcs.strict.call(receiver, i);
console.log("strict", receiver.length, Date.now() - start, sum);
sum = 0; start = Date.now();
for (let i = 0; i < n; i++) sum += Object(receiver).length;
console.log("Object", receiver.length, Date.now() - start, sum);

const first = funcs.capture.call(receiver);
const second = funcs.capture.apply(receiver, []);
if (typeof first !== "object" || first === second || first.length !== receiver.length) {
  throw new Error("sloppy calls must create distinct String wrappers");
}
first.extra = 7;
if (second.extra !== undefined) throw new Error("receiver state leaked");
delete String.prototype.bench9810;
