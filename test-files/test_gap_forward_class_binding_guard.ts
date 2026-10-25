// A function-nested class that keeps the shared template for its first
// evaluation (static state, no heritage, a scope that runs more than once) and
// that captures enclosing locals is read through a binding pre-registered for
// the closure that precedes the declaration. The compiler refreshes the class's
// capture snapshot after every assignment of a captured local and before every
// return, and that refresh asks the binding which evaluation it holds. It runs
// before the declaration initializes the binding, so it must not raise the
// dead-zone ReferenceError: only the program's own reads do. Every line is
// compared with node. Each case runs on its own so that one failure does not
// hide the others (a case that throws prints `threw`).

function attempt(label, fn) {
  try {
    console.log(label, JSON.stringify(fn()));
  } catch (e) {
    console.log(label, 'threw', e && e.name);
  }
}

// 1. a captured const is initialized between the closure and the declaration
function initialized(first) {
  const read = () => C;
  const n = first * 10;
  class C {
    static count = 0;
    value() {
      return n;
    }
  }
  return { read, C };
}
attempt('initialized', () => {
  const one = initialized(1);
  const two = initialized(2);
  return [
    one.read() === one.C,
    two.read() === two.C,
    one.C !== two.C,
    new one.C().value(),
    new two.C().value(),
  ];
});

// 2. a captured let is assigned between the closure and the declaration
function assigned(first) {
  const read = () => C;
  let n = first;
  n = n + 1;
  class C {
    static count = 0;
    value() {
      return n;
    }
  }
  return { read, C };
}
attempt('assigned', () => {
  const one = assigned(1);
  const two = assigned(5);
  return [
    one.read() === one.C,
    two.read() === two.C,
    new one.C().value(),
    new two.C().value(),
  ];
});

// 3. a return before the declaration, and one after it
function early(stop) {
  const read = () => C;
  let n = 1;
  if (stop) return read;
  n = 2;
  class C {
    static count = 0;
    value() {
      return n;
    }
  }
  return read;
}
attempt('early-return', () => {
  const read = early(true);
  let before;
  try {
    read();
    before = 'no error';
  } catch (e) {
    before = e.name;
  }
  return [typeof read, before];
});
attempt('late-return', () => {
  const C = early(false)();
  return [typeof C, new C().value()];
});

// 4. the program's own read before the declaration still raises the error
function dead(first) {
  const read = () => C;
  const n = first;
  let seen;
  try {
    read();
    seen = 'no error';
  } catch (e) {
    seen = e.name;
  }
  class C {
    static count = 0;
    value() {
      return n;
    }
  }
  return [seen, read() === C, new C().value()];
}
attempt('dead-zone', () => [dead(1), dead(2)]);
