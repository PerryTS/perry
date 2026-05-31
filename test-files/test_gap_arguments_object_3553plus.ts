// Gap test: ordinary function `arguments` object materialization (#3553).
// Covers indexed access, .length, iteration, spread, Array.from, the
// [object Arguments] toStringTag, and Object.keys. Compared byte-for-byte
// against `node --experimental-strip-types`.

function lenFn() {
  return arguments.length;
}
console.log(lenFn(1, 2, 3));
console.log(lenFn());
console.log(lenFn("a", "b"));

function idxFn() {
  return arguments[0] + ":" + arguments[1];
}
console.log(idxFn("x", "y"));

function spreadFn() {
  return [...arguments];
}
console.log(spreadFn(1, 2, 3));

function fromFn() {
  return Array.from(arguments);
}
console.log(fromFn(4, 5));

function tagFn() {
  return Object.prototype.toString.call(arguments);
}
console.log(tagFn(1));

function typeFn() {
  return typeof arguments;
}
console.log(typeFn(1));

function iterFn() {
  const out: number[] = [];
  for (const x of arguments) out.push((x as number) * 2);
  return out;
}
console.log(iterFn(1, 2, 3));

function keysFn() {
  return Object.keys(arguments);
}
console.log(keysFn("a", "b", "c"));

// arguments.length reflects ALL passed args, not the declared arity.
function dualFn(a: number, b: number) {
  return arguments.length;
}
console.log(dualFn(1, 2, 3, 4));
console.log(dualFn(1));
