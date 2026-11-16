// DEC 250: Function.prototype.bind is locked in a compiled program, so the patch is refused and every borrowed bind runs the builtin.
// tests/parity/expected holds Perry's output.
function locked(f: () => void) { try { f(); } catch (e) { console.log("DEC 250: " + (e as Error).message); } }
const savedBind = Function.prototype.bind;
const source = process.argv[2] || "Function.prototype.bind = function () { return function () { return 703; }; }";
locked(() => new Function(source)());
const a = Math.max.bind(null, 2)(3);
const b = Promise.resolve.bind(Promise)(1);
const restore = process.argv[3] || "target.bind = value";
locked(() => new Function("target", "value", restore)(Function.prototype, savedBind));
console.log(a, b);
