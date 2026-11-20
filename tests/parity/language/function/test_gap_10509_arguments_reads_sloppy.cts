// #10509, sloppy half (CommonJS, so every function below is non-strict): a
// function that only reads `arguments` keeps the caller's argument bundle
// instead of building a mapped Arguments object. That is only sound while
// every parameter the mapped object would alias still holds its incoming
// value, and a non-element key such as `"callee"` must still see the
// function itself. The strict half is `test_gap_10509_arguments_reads.ts`.
//
// Compared byte-for-byte against `node --experimental-strip-types`.

// Read-only, parameters untouched: the elided shape.
function sloppyRead(a: any, b: any): any {
  const callee = "cal" + "lee";
  return [
    arguments.length,
    arguments[0],
    arguments[1],
    arguments[2],
    typeof arguments[callee],
    arguments[callee] === sloppyRead,
    a,
    b,
  ].join("|");
}
console.log("read", (sloppyRead as any)("x", "y"), (sloppyRead as any)("x", "y", "z"));
console.log("read short", (sloppyRead as any)("only"));

// The Babel default-parameter output, sloppy.
function mergeSloppy(): any {
  var obj = arguments.length > 0 && arguments[0] !== undefined ? arguments[0] : { fresh: 1 };
  var defaults = arguments.length > 1 ? arguments[1] : undefined;
  return JSON.stringify(obj) + " " + String(defaults);
}
console.log("merge", mergeSloppy(), (mergeSloppy as any)({ a: 1 }), (mergeSloppy as any)(undefined, 2));

// Writes to an aliased parameter are visible through `arguments`.
function writeParam(a: any): any {
  a = 9;
  return arguments[0];
}
console.log("write", (writeParam as any)(1));
function updateParam(a: any): any {
  a++;
  return [arguments[0], arguments.length].join("|");
}
console.log("update", (updateParam as any)(1));
function closureWritesParam(a: any): any {
  const set = () => {
    a = "from closure";
  };
  set();
  return arguments[0];
}
console.log("closure write", (closureWritesParam as any)("orig"));

// `var a` re-declaring a parameter is the same binding.
function varParam(a: any): any {
  var a = 5;
  return arguments[0];
}
function varParamInBranch(a: any): any {
  if (a) {
    var a = 7;
  }
  return arguments[0];
}
console.log("var", (varParam as any)(1), (varParamInBranch as any)(1));

// Only unaliased parameters are written: reads are still exact.
function readsWithLocal(a: any, b: any): any {
  let t = arguments[0];
  t = t + "!";
  return [t, arguments[1], a, b].join("|");
}
console.log("local", (readsWithLocal as any)("p", "q"));

// `callee` of a function expression is the closure itself. The key arrives
// as a parameter so it stays a computed read.
const expr = function (k: any): any {
  return [arguments[k] === expr, arguments[0]].join("|");
};
console.log("expr callee", (expr as any)("callee"));

// Loops over a sloppy object.
function sum(): number {
  let n = 0;
  for (let i = 0; i < arguments.length; i++) n += arguments[i];
  return n;
}
console.log("sum", (sum as any)(1, 2, 3, 4));

// A nested arrow reads the enclosing function's `arguments`.
function outer(a: any): any {
  const inner = () => arguments[0] + "/" + arguments.length;
  return inner();
}
console.log("outer", (outer as any)("o", 2));
