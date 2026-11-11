// Issue #12632 (shared routing): a throw delivered to a suspended generator,
// or executing inside it, leaves every try statement between it and the
// handler that catches it — so a non-yielding `finally` nested inside the
// handler's try must run first. Nested tries can share a start state, so the
// handler order is the nesting order, not the start state.

// 1. .throw() into an inner try/finally inside an outer try/catch.
function* g1() {
  try {
    try {
      yield 1;
    } finally {
      console.log("1 inner finally");
    }
  } catch (e) {
    console.log("1 outer catch", (e as Error).message);
  }
  return "1 done";
}
const a = g1();
a.next();
console.log("1:", JSON.stringify(a.throw(new Error("x1"))));

// 2. A throw executing after the resume, same nesting.
function* g2() {
  try {
    try {
      yield 1;
      throw new Error("t2");
    } finally {
      console.log("2 inner finally");
    }
  } catch (e) {
    console.log("2 outer catch", (e as Error).message);
  }
}
const b = g2();
b.next();
console.log("2:", JSON.stringify(b.next()));

// 3. Three levels: two non-yielding finallys run innermost first, then the catch.
function* g3() {
  try {
    try {
      try {
        yield 1;
      } finally {
        console.log("3 finally a");
      }
    } finally {
      console.log("3 finally b");
    }
  } catch (e) {
    console.log("3 catch", (e as Error).message);
  }
  return "3 done";
}
const c = g3();
c.next();
console.log("3:", JSON.stringify(c.throw(new Error("x3"))));

// 4. .return() through a non-yielding inner finally into a yielding outer one.
function* g4() {
  try {
    try {
      yield 1;
    } finally {
      console.log("4 inner finally");
    }
  } finally {
    yield 2;
    console.log("4 outer finally");
  }
}
const d = g4();
d.next();
console.log("4:", JSON.stringify(d.return(44)));
console.log("4:", JSON.stringify(d.next()));

// 5. A throw in a try's first state after a yield, no handler.
function* g5() {
  yield 0;
  try {
    throw new Error("t5");
    yield 1;
  } finally {
    console.log("5 finally");
  }
}
const e5 = g5();
e5.next();
try {
  e5.next();
} catch (e) {
  console.log("5: threw", (e as Error).message);
}
