// A class declaration that an earlier closure reads binds a class value of its
// own whenever the declaration evaluates to one, even in a scope that runs once
// (an immediately invoked function, a function called once, module code): a
// class that captures enclosing locals and has no static state, and a class
// with a computed method key. The earlier closure must read that class, not the
// shared template. Every line is compared with node.
//
// Perry treats a position as run-once only when it is structurally so: module
// code, an immediately invoked function or a function declared and called once
// from such a position. Those cases therefore sit directly in module-level
// `try` blocks rather than inside a callback, which is a position that may run
// any number of times. Each case runs on its own so that one failure does not
// hide the others (a case that throws prints `threw`).

function report(scope) {
  const Read = scope.read();
  return [
    Read === scope.C,
    new scope.C().value(),
    new Read().value(),
    new scope.C() instanceof Read,
  ];
}

function outcome(label, value) {
  console.log(label, typeof value === 'string' ? value : JSON.stringify(value));
}

function threw(e) {
  return 'threw ' + (e && e.name);
}

// 1. an immediately invoked function, a captured local
try {
  outcome(
    'captured-iife',
    report(
      (() => {
        const n = 1;
        const read = () => C;
        class C {
          value() {
            return n;
          }
        }
        return { read, C };
      })(),
    ),
  );
} catch (e) {
  outcome('captured-iife', threw(e));
}

// 2. a function called once, a captured parameter
function calledOnce(n) {
  const read = () => C;
  class C {
    value() {
      return n;
    }
  }
  return { read, C };
}
try {
  outcome('captured-called-once', report(calledOnce(3)));
} catch (e) {
  outcome('captured-called-once', threw(e));
}

// 3. a captured local assigned after the closure
try {
  outcome(
    'captured-assigned',
    report(
      (() => {
        const read = () => C;
        let n = 0;
        n = 4;
        class C {
          value() {
            return n;
          }
        }
        return { read, C };
      })(),
    ),
  );
} catch (e) {
  outcome('captured-assigned', threw(e));
}

// 4. a computed method key, no captures
try {
  const scope = (() => {
    const read = () => K;
    const key = 'dynamic';
    class K {
      [key]() {
        return 7;
      }
    }
    return { read, K };
  })();
  const Read = scope.read();
  outcome('computed-method', [
    Read === scope.K,
    new scope.K().dynamic(),
    new Read().dynamic(),
  ]);
} catch (e) {
  outcome('computed-method', threw(e));
}

// 5. a block of module code, a captured local
let blockRead;
let blockClass;
{
  const n = 5;
  blockRead = () => B;
  class B {
    value() {
      return n;
    }
  }
  blockClass = B;
}
try {
  outcome('captured-block', report({ read: blockRead, C: blockClass }));
} catch (e) {
  outcome('captured-block', threw(e));
}

// 6. controls: a scope that runs twice keeps one class per call, and a class
// with static state in a scope that runs once is the shared class (it has a
// name of its own: Perry resolves a second class declaration of an already
// registered name to the first one, whatever the scope)
function twice(n) {
  const read = () => C;
  class C {
    value() {
      return n;
    }
  }
  return { read, C };
}
try {
  const one = twice(1);
  const two = twice(2);
  outcome('captured-twice', [report(one), report(two), one.C !== two.C]);
} catch (e) {
  outcome('captured-twice', threw(e));
}
try {
  outcome(
    'static-once',
    report(
      (() => {
        const n = 6;
        const read = () => Counted;
        class Counted {
          static count = 0;
          value() {
            return n;
          }
        }
        return { read, C: Counted };
      })(),
    ),
  );
} catch (e) {
  outcome('static-once', threw(e));
}
