// Issue #4374: generator `.return(v)` and `.throw(e)` must run pending
// `try`/`finally` (and `.throw` must route through `catch`) when the generator
// is suspended at a `yield` inside a `try`. Previously `.return` completed
// immediately (skipping finally) and `.throw` deferred finally to the next
// `.next()` and returned the wrong iter-result.

// 1. `.return(v)` runs the pending finally, then completes with {value: v}.
function* g1() {
  try {
    yield 1;
    yield 2;
  } finally {
    console.log("f1");
  }
}
const a = g1();
a.next();
console.log("1:", JSON.stringify(a.return(99)));

// 2. A `return` inside finally supersedes the `.return` value.
function* g2() {
  try {
    yield 1;
  } finally {
    return 42;
  }
}
const b = g2();
b.next();
console.log("2:", JSON.stringify(b.return(7)));

// 3. Nested finally blocks run innermost-first.
function* g3() {
  try {
    try {
      yield 1;
    } finally {
      console.log("inner");
    }
  } finally {
    console.log("outer");
  }
}
const c = g3();
c.next();
console.log("3:", JSON.stringify(c.return(5)));

// 4. `.return` on a not-yet-started generator does NOT run finally.
function* g4() {
  try {
    yield 1;
  } finally {
    console.log("should-not-print");
  }
}
const d = g4();
console.log("4:", JSON.stringify(d.return(8)));

// 5. `.return` on an exhausted generator does not re-run finally.
function* g5() {
  try {
    yield 1;
  } finally {
    console.log("f5once");
  }
}
const e = g5();
e.next();
e.next();
console.log("5:", JSON.stringify(e.return(3)));

// 6. `.throw` routes through catch, runs finally, completes within the call.
function* g6() {
  try {
    yield 1;
  } catch (err) {
    console.log("caught", err);
  } finally {
    console.log("f6");
  }
}
const f = g6();
f.next();
console.log("6:", JSON.stringify(f.throw("boom")));
console.log("6b:", JSON.stringify(f.next()));

// 7. `.throw` on a catch-less try/finally runs finally then propagates.
function* g7() {
  try {
    yield 1;
  } finally {
    console.log("f7");
  }
}
const h = g7();
h.next();
try {
  h.throw(new Error("x"));
} catch (err) {
  console.log("7 propagated:", (err as Error).message);
}

// 8. After catch handles, the generator continues to subsequent yields.
function* g8() {
  try {
    yield 1;
  } catch (err) {
    console.log("caught8");
  }
  yield 2;
  yield 3;
}
const i = g8();
i.next();
console.log("8a:", JSON.stringify(i.throw("e")));
console.log("8b:", JSON.stringify(i.next()));
console.log("8c:", JSON.stringify(i.next()));

// 9. An uncaught `.throw` closes the generator.
function* g9() {
  yield 1;
  yield 2;
}
const j = g9();
j.next();
try {
  j.throw("nope");
} catch (err) {
  console.log("9 propagated:", err);
}
console.log("9b:", JSON.stringify(j.next()));

// 10. A catch that re-throws still runs the finally before propagating.
function* g10() {
  try {
    yield 1;
  } catch (err) {
    throw new Error("rethrown");
  } finally {
    console.log("f10");
  }
}
const k = g10();
k.next();
try {
  k.throw("x");
} catch (err) {
  console.log("10:", (err as Error).message);
}

// 11. throw caught by an inner try, return runs the outer finally.
function* g11() {
  try {
    try {
      yield 1;
    } catch (err) {
      console.log("inner-catch");
    }
    yield 2;
  } finally {
    console.log("outer-fin");
  }
}
const m = g11();
m.next();
console.log("11a:", JSON.stringify(m.throw("z")));
console.log("11b:", JSON.stringify(m.return(9)));
