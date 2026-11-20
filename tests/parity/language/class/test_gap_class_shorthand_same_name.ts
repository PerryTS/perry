// Two functions each declare their own `class C`; Perry registers the second
// one under a scope-local key. A shorthand property `{ C }` must read the class
// its own scope declared, exactly like the identifier `C` does, not the first
// class registered under that name. The run-once case also has an earlier
// closure reading the class and a captured parameter: the closure, the
// shorthand and `new` must all see the same class carrying that capture.
// Every line is compared with node.

function first() {
  class C {
    v() {
      return 1;
    }
  }
  return { C };
}
function second() {
  class C {
    v() {
      return 2;
    }
  }
  return { C };
}
const x = first();
const y = second();
console.log('plain', new x.C().v(), new y.C().v(), x.C !== y.C);

function third() {
  class C {
    v() {
      return 3;
    }
  }
  return { C, spread: { ...{ C } }, other: 0 };
}
const z = third();
console.log('mixed', new z.C().v(), new z.spread.C().v(), z.C === z.spread.C, z.C !== x.C);

function capturedOnce(n) {
  const read = () => C;
  class C {
    v() {
      return n;
    }
  }
  return { read, C };
}
const s = capturedOnce(7);
console.log('captured', s.read() === s.C, new s.C().v(), new (s.read())().v(), s.C !== x.C);

// The `{ cause }` option of `new Error` is a shorthand too. The other
// `class cause` comes first, so it is the one registered under the name.
function otherCause() {
  class cause {
    v() {
      return 8;
    }
  }
  return cause;
}
function errorCause() {
  class cause {
    v() {
      return 9;
    }
  }
  const e = new Error('m', { cause });
  return [new e.cause().v(), e.cause === cause];
}
otherCause();
console.log('error-cause', JSON.stringify(errorCause()));
