// Runtime source replaces the global Math binding with an ordinary object; a
// direct Math call must read the new binding. Then JSON is redefined through
// Object.defineProperty on globalThis.
console.log(Math.abs(-3), Math.floor(2.5), JSON.stringify([1]));
const body =
    process.argv[2] ||
    "globalThis.Math = { abs: function (v) { return 'abs' + v; }, floor: Math.floor };";
new Function(body)();
console.log(Math.abs(-3), Math.floor(2.5));
function absOf(v: number) {
    return Math.abs(v);
}
console.log(absOf(-7));
const body2 =
    process.argv[3] ||
    "Object.defineProperty(globalThis, 'JSON', { value: { stringify: function () { return 'J'; } }, configurable: true, writable: true });";
new Function(body2)();
console.log(JSON.stringify([1]));
