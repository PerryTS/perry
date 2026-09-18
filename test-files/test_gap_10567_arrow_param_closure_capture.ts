// #10567: a closure created inside an arrow function captured the arrow's
// OWN parameter by first-call value -- a later call to the same arrow still
// saw the FIRST call's argument inside the nested closure.
//
// Root cause: `closure_local_inline` (crates/perry-transform/src/closure_local_inline.rs)
// beta-reduces a `let f = (a, b) => <single return expr>` local that is only
// ever called, cloning the return expression fresh per call site and
// substituting each parameter with that call's own argument via
// `substitute_locals`. When a parameter is read inside a NESTED closure
// (e.g. `(f, isOpt) => arr.forEach(([k, v]) => check(k, v, isOpt))`),
// `substitute_locals`'s `Expr::Closure` arm bakes a non-`LocalGet` argument
// straight into that nested closure's body and drops it from the closure's
// `captures` list -- but never mints a fresh `func_id` for the rewritten
// closure literal. Codegen compiles exactly one body per `func_id`
// (whichever `Expr::Closure` occurrence its module-wide scan sees first), so
// every call site's clone of the nested closure kept sharing the SAME
// `func_id`: with more than one call site, only the first-seen clone's
// baked-in argument was ever compiled, and every other call silently ran it
// too.

function validate(fields: any = {}, optFields: any = {}) {
  function check(name: string, t: string, isOpt: boolean) {
    console.log(name, t, isOpt);
  }
  const iter = (f: any, isOpt: boolean) =>
    Object.entries(f).forEach(([k, v]) => check(k, v as string, isOpt));
  iter(fields, false);
  iter(optFields, true); // the inner arrow must see isOpt === true here
}
validate({ x: "number" }, { a: "boolean" });

// Several params: the CAPTURED one is not the first, and not the last.
function severalParams() {
  const combine = (prefix: string, mid: number, tag: boolean) =>
    [1, 2].forEach((v) => console.log(prefix, mid, v, tag));
  combine("A", 1, false);
  combine("B", 2, true);
}
severalParams();

// Nested arrows: outer -> middle (forwards) -> inner (captures outer's own
// param transitively, two closure boundaries away).
function nestedArrows() {
  const outer = (tag: string) => {
    const middle = () => [10, 20].forEach((v) => console.log(tag, v));
    return middle();
  };
  outer("first");
  outer("second");
}
nestedArrows();

// Arrow inside a class METHOD: same shape as `iter` above, but declared
// inside a method body rather than a plain function.
class Validator {
  run() {
    const iter = (isOpt: boolean) =>
      [1, 2].forEach((v) => console.log("method", v, isOpt));
    iter(false);
    iter(true);
  }
}
new Validator().run();

// Capturing the param by WRITE inside the nested closure (a multi-statement
// arrow body, so it never becomes a `closure_local_inline` candidate at
// all -- this is a control that should keep working, matching the
// already-correct `outer`/`inner` shape from the original issue).
function byWrite() {
  const make = (isOpt: boolean) => {
    let seen = isOpt;
    [1, 2].forEach((v) => {
      seen = seen || v > 1;
    });
    return seen;
  };
  console.log(make(false), make(true));
}
byWrite();

// Plain-function control that already worked on Perry: a directly-invoked
// inner closure, and a function declaration called twice.
const calls: any[] = [];
function outer(tag: string) {
  const inner = (v: number) => calls.push(tag + ":" + v);
  inner(1);
}
outer("A");
outer("B");
console.log(calls.join(","));

function twice(p: boolean) {
  const g = () => p;
  return g();
}
console.log(twice(false), twice(true));
