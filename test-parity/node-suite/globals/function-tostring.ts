// #4101: Function.prototype.toString reconstructs a function's source text
// (instead of the generic "[object Object]"), and Function.prototype.toString
// invoked on a non-function throws a TypeError (the brand check).
//
// Every function whose source is printed here is intentionally untyped so the
// output stays byte-identical under Node's `--experimental-strip-types` (which
// blanks type annotations) and Perry's verbatim source retention.

function foo(a) { return a; }
console.log(foo.toString());

const bar = (a) => a * 2;
console.log(bar.toString());

const baz = function namedBaz(x) { return x + 1; };
console.log(baz.toString());

const multi = function (n) {
  let total = 0;
  for (let i = 0; i < n; i++) {
    total += i;
  }
  return total;
};
console.log(multi.toString());

// Reflective form resolves the same source.
console.log(Function.prototype.toString.call(foo));
console.log(Function.prototype.toString.apply(bar));

// Brand check: a non-function receiver throws a TypeError.
try {
  Function.prototype.toString.call({});
  console.log("no throw (object)");
} catch (e) {
  console.log(e.constructor.name + ": " + e.message);
}

try {
  Function.prototype.toString.call(42);
  console.log("no throw (number)");
} catch (e) {
  console.log(e.constructor.name);
}

// Object.prototype.toString stays lenient — no brand check, no throw.
console.log(Object.prototype.toString.call({}));
