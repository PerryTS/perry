// IteratorRecord.[[NextMethod]] is read once, before the first step.
let nextReads = 0;
const it = {
  i: 0,
  [Symbol.iterator]() { return this; },
  get next() { nextReads++; const self = this; return function () { return self.i < 3 ? { value: self.i++, done: false } : { value: undefined, done: true }; }; },
};
function f(...a) { return a.join(","); }
console.log(f(...it), nextReads);
