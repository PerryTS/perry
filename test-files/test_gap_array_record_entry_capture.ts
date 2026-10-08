// These run before any intrinsic override can revoke the entry proof.
const own: any = [1,2,3];
let ownCalls = 0;
let text = "";
for (const x of own) {
  text += x;
  if (x === 1) own[Symbol.iterator] = function() { ownCalls++; throw 8; };
}
console.log("own-entry", text, ownCalls);
let getterCloses = 0;
const family: any = Object.getPrototypeOf([].values());
family.return = function() { getterCloses++; return {}; };
const failing: any = [];
Object.defineProperty(failing, "0", { get() { throw 17; }, configurable: true });
failing.length = 1;
try { for (const x of failing) console.log("unreachable", x); }
catch (e) { console.log("step-get-fail", e, getterCloses); }
try { const [,x] = failing; console.log("unreachable", x); }
catch (e) { console.log("elision-get-fail", e, getterCloses); }
delete family.return;
// Replacing the source's iteration method after entry cannot change the record.
const original = Array.prototype[Symbol.iterator];
const source: any = [4,5,6];
let protoCalls = 0;
text = "";
for (const x of source) {
  text += x;
  if (x === 4) Array.prototype[Symbol.iterator] = function() { protoCalls++; throw 9; };
}
Array.prototype[Symbol.iterator] = original;
console.log("prototype-entry", text, protoCalls);
