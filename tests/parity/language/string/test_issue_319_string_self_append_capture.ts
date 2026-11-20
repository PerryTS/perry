// Regression test for #319 — `s = s + t` on a closure-captured
// string-typed local aborted codegen with
//   `string self-append: local N not in scope`
// because the self-append fast path's `ctx.locals.get(id)` lookup
// whiffed for captured locals (which live in the closure env, not
// in an alloca slot). Fix: gate the fast path with
// `closure_captures` / `boxed_vars` / `locals.contains_key` so
// captured + boxed cases fall through to the regular store path.

function makeAppender() {
  let s = "";
  return function step(t: string) {
    s = s + t;
    return s;
  };
}

const a = makeAppender();
console.log(a("foo"));
console.log(a("bar"));
console.log(a("baz"));

// Independent appender — verify each closure has its own captured slot.
const b = makeAppender();
console.log(b("X"));
console.log(b("Y"));

// Original captured `a` still works after `b` ran.
console.log(a("!"));

// Self-append with non-string rhs (coerced) — must still hit the
// regular path, not the fast path's bail.
function makeMixedAppender() {
  let s = "";
  return function step(n: number) {
    s = s + n;
    return s;
  };
}

const m = makeMixedAppender();
console.log(m(1));
console.log(m(2));
console.log(m(3));

// Plain (non-captured) string self-append still hits the fast path.
function plainBuild() {
  let s = "";
  for (let i = 0; i < 3; i++) {
    s = s + "x";
  }
  return s;
}
console.log(plainBuild());
