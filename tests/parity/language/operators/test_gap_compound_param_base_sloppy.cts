// A sloppy function with a simple parameter list has a MAPPED arguments
// object: `arguments[0] = v` assigns the first parameter. A compound member
// assignment `a[k] op= v` must still write the receiver read before the key
// and the right-hand side ran.
//
// CommonJS, so functions are sloppy unless they say "use strict". No
// `require` on purpose: the module stays a plain script.
// Compared byte-for-byte against `node --experimental-strip-types`.

function viaArgumentsInRhs(a: any, b: any) {
  a.n += ((arguments[0] = b), 1);
  a.n += 100;
  return [a === b, arguments[0] === b];
}
{
  const x = { n: 1 }, y = { n: 50 };
  const r = viaArgumentsInRhs(x, y);
  console.log("rhs:", x.n, y.n, r.join(","));
}

function viaArgumentsInKey(a: any, b: any) {
  a[((arguments[0] = b), "n")] *= 10;
  return a === b;
}
{
  const x = { n: 2 }, y = { n: 7 };
  console.log("key:", viaArgumentsInKey(x, y), x.n, y.n);
}

// The arguments object escapes to a helper that writes it.
function poke(args: any, v: any) { args[0] = v; return 1; }
function viaEscapedArguments(a: number[], b: number[]) {
  for (let i = 0; i < 2; i++) a[i] += poke(arguments, b);
  return a === b;
}
{
  const x = [1, 2], y = [10, 20];
  console.log("escaped:", viaEscapedArguments(x, y), x.join(","), y.join(","));
}

// A strict function's arguments object is unmapped: the parameter keeps its
// value.
function strictArguments(a: any, b: any) {
  "use strict";
  a.n += ((arguments[0] = b), 1);
  a.n += 1000;
  return a === b;
}
{
  const x = { n: 1 }, y = { n: 50 };
  console.log("strict:", strictArguments(x, y), x.n, y.n);
}

// A var re-declaration with an initializer rebinds the parameter.
function redeclared(a: any, b: any) {
  a.n += 1;
  var a = b;
  a.n += 1;
  return a === b;
}
{
  const x = { n: 1 }, y = { n: 50 };
  console.log("var:", redeclared(x, y), x.n, y.n);
}
