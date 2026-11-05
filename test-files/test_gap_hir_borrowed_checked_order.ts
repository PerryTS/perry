let effects = 0;
function receiver() { effects++; return null; }
console.log((function () { return 11; }).call(receiver()), effects);
console.log(Math.max.call(receiver(), 2, 3), effects);
const a: any = [1, 2];
a.slice = function () { return "own"; };
console.log(Array.prototype.slice.call(a).join(","));
const saved = Function.prototype.call;
function patch() {
  const code = process.argv[2] || "Function.prototype.call = function () { return 999; }";
  new Function(code)();
  return null;
}
const snapshot = (function () { return 12; }).call(patch());
const restore = process.argv[3] || "target.call = value";
new Function("target", "value", restore)(Function.prototype, saved);
console.log(snapshot);
const savedSlice = Array.prototype.slice;
function patchSlice() {
  Array.prototype.slice = function () { return [999]; };
  return [3, 4];
}
const arraySnapshot = Array.prototype.slice.call(patchSlice());
Array.prototype.slice = savedSlice;
console.log(arraySnapshot.join(","));
