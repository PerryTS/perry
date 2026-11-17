// A patched %ArrayIteratorPrototype%.next is observed by call spread (arrays and arguments).
// DEC 250: %ArrayIteratorPrototype%.next is locked in a compiled program, so the patch is refused and spread steps the builtin iterator.
// tests/parity/expected holds Perry's output.
function locked(f: () => void) { try { f(); } catch (e) { console.log("DEC 250: " + (e as Error).message); } }
const AIP = Object.getPrototypeOf([][Symbol.iterator]());
const orig = AIP.next;
function f(a, b, c, d) { return [a, b, c, d].join(","); }
class V { set(a, b, c, d) { return [a, b, c, d].join(","); } }
function viaArgs() { return f(...arguments); }
const arr = [1, 2, 3];
console.log(f(...arr, 9), new V().set(...arr, 9), viaArgs(4, 5, 6));
let steps = 0;
locked(() => { AIP.next = function () { steps++; const r = orig.call(this); if (!r.done) r.value = -r.value; return r; }; });
console.log(f(...arr, 9), new V().set(...arr, 9), viaArgs(4, 5, 6), steps);
locked(() => { AIP.next = orig; });
console.log(f(...arr, 9), steps);
