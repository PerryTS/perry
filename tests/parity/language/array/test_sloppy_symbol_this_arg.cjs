// A sloppy callback a builtin calls with a Symbol thisArg sees its wrapper
// object; a strict one sees the symbol itself.
const sym = Symbol("s");

function kind() { return typeof this; }
function wraps() { return this instanceof Symbol && this.valueOf() === sym; }
function strictKind() { "use strict"; return typeof this + ":" + (this === sym); }

console.log([0].map(kind, sym)[0], kind.call(sym));
console.log([0].map(wraps, sym)[0], [0].map(strictKind, sym)[0]);

const seen = [];
[0].forEach(function () { seen.push(typeof this); }, sym);
[0].forEach(function () { "use strict"; seen.push(typeof this); }, sym);
console.log("forEach", seen.join(" "));

console.log("filter", [0, 1].filter(wraps, sym).length);
console.log("some", [0].some(wraps, sym), "every", [0].every(wraps, sym));
console.log("find", [7].find(wraps, sym), "findIndex", [7].findIndex(wraps, sym));
console.log("Array.from", Array.from([0], kind, sym)[0], Array.from([0], strictKind, sym)[0]);
console.log("typed", Array.from(new Uint8Array(1).map(function () { return wraps.call(this) ? 1 : 0; }, sym))[0]);

function self() { return this; }
const [first, second] = [0, 1].map(self, sym);
console.log("fresh wrapper", first !== second, first.valueOf() === second.valueOf());

const setSeen = [];
new Set([1]).forEach(function () { setSeen.push(typeof this); }, sym);
new Map([[1, 1]]).forEach(function () { setSeen.push(typeof this); }, sym);
console.log("collections", setSeen.join(" "));

Array.fromAsync([0], kind, sym)
  .then((sloppy) => Array.fromAsync([0], strictKind, sym).then((strict) => {
    console.log("Array.fromAsync", sloppy[0], strict[0]);
  }));
