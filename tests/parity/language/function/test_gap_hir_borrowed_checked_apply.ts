import path from "node:path";
// DEC 250: Function.prototype.apply is locked in a compiled program, so the patch is refused and every borrowed apply runs the builtin.
// tests/parity/expected holds Perry's output.
function locked(f: () => void) { try { f(); } catch (e) { console.log("DEC 250: " + (e as Error).message); } }
const savedApply = Function.prototype.apply;
const source = process.argv[2] || "Function.prototype.apply = function () { return 702; }";
locked(() => eval(source));
const math = Math.max.apply(null, [2, 3]);
const dynamic = Math.min.apply(null, [2, 3].slice());
const array = Array.prototype.slice.apply([1, 2], [0]);
const native = path.join.apply(null, ["a", "b"]);
const restore = process.argv[3] || "target.apply = value";
locked(() => new Function("target", "value", restore)(Function.prototype, savedApply));
console.log(math, dynamic, array, native);
