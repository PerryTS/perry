// A patched Array.prototype[Symbol.iterator] is observed by call spread.
// DEC 250: Array.prototype[Symbol.iterator] is locked in a compiled program, so the patch is refused and spread reads the builtin iterator.
// tests/parity/expected holds Perry's output.
function locked(f: () => void) { try { f(); } catch (e) { console.log("DEC 250: " + (e as Error).message); } }
function f(a, b, c, d) { return [a, b, c, d].join(","); }
class V { set(a, b, c, d) { return [a, b, c, d].join(","); } }
const arr = [1, 2, 3];
console.log(f(...arr, 9), new V().set(...arr, 9));
const orig = Array.prototype[Symbol.iterator];
locked(() => { Array.prototype[Symbol.iterator] = function* () { for (const x of orig.call(this)) yield x * 10; }; });
console.log(f(...arr, 9), new V().set(...arr, 9), Math.max(...arr));
locked(() => { Array.prototype[Symbol.iterator] = orig; });
console.log(f(...arr, 9), new V().set(...arr, 9));
