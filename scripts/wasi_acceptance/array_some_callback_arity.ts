// `arr.some(arrow)` with a captureless arrow takes a runtime helper for every
// receiver the compiler's inline loop does not admit. The helper must apply
// the arrow's own declared parameter count: wasm checks the type of an
// indirect call, so passing (value, index, array) to a body that declares
// fewer parameters traps with "indirect call type mismatch".
const accessor = [1, 2, 3];
Object.defineProperty(accessor, 1, { get() { return 20; }, enumerable: true, configurable: true });
console.log(accessor.some((x) => x > 10), accessor.some((x) => x > 100));
console.log(accessor.some((x, i) => i === 2), accessor.some((x, i, a) => a.length === 3));
console.log(accessor.some(() => true), [].some(() => true));
const holes = [1, , 3];
console.log(holes.some((x) => x === undefined), holes.some((x, i) => i === 1));
Array.prototype[1] = 7;
console.log(holes.some((x) => x === 7), holes.some((x, i) => i === 3));
delete Array.prototype[1];
console.log(holes.some((x) => x === 7));
const routes = [{ keep: false }, { keep: true }];
console.log(routes.some((route) => route.keep), routes.some((route) => route.drop));
