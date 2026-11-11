// A class declaration naming a class declared LATER in the same scope, while
// an earlier closure also reads that class (`const read = () => C`), must see
// that class once it is declared, whichever way the class is used afterwards:
// retained from an inner block whose bindings are gone, evaluated several
// times, instantiated before the later class exists, or called as a
// generator's parameter default. A closure made while a class is defined (a
// computed key, the heritage, a static initializer) reads the later class's
// own binding, whatever scope the class sits in (a catch clause, an inner
// block that shadows an outer pending class of the same name) and whatever the
// later class is: a plain class, one that captures a parameter, a subclass of
// such a class, one that extends a built-in. The closure that reads it may sit
// in a class declaration's computed key, or be created in a loop body that a
// `using` declaration wraps in a `try`, or call `eval` directly (a string can
// name any later class), or call it in a member body while the later class is
// pending, or in the function body itself, where it creates the closure that
// reads the class. Every line is compared with node. Each
// case runs on its own so that one failure does not hide the others (a case
// that throws prints `threw`).
//
// The later class is a plain class in most cases: a class declaration names
// it through the shared template, which is the class itself for the first
// evaluation (the loops below record that evaluation only).

function outcome(label, value) {
  console.log(label, typeof value === 'string' ? value : JSON.stringify(value));
}

function attempt(label, fn) {
  try {
    outcome(label, fn());
  } catch (e) {
    outcome(label, 'threw ' + (e && e.name));
  }
}

// The class names `x` from an inner block and the later `C`; once the block is
// left `x` is out of scope while `C` is still uninitialized.
attempt('inner-block-capture', function () {
  const out = [];
  for (let i = 0; i < 1; i++) {
    const read = () => C1;
    const classes = [];
    {
      const x = i;
      class Holder1 {
        value() {
          return [x, C1];
        }
      }
      classes.push(Holder1);
    }
    class C1 {}
    const H = classes[0];
    out.push([new H().value()[1] === C1, read() === C1]);
  }
  return out;
});

// The inner block's class is built again after the block, with its own capture.
attempt('inner-block-capture-twice', function () {
  const out = [];
  for (let i = 0; i < 1; i++) {
    const read = () => C2;
    const classes = [];
    for (let k = 0; k < 2; k++) {
      const x = k;
      class Holder2 {
        value() {
          return [x, C2];
        }
      }
      classes.push(Holder2);
    }
    class C2 {}
    out.push(classes.map((H) => new H().value()).map(([x, c]) => [x, c === C2]));
    out.push(read() === C2);
  }
  return out;
});

// The declaration is evaluated twice before the later class initializes; both
// retained classes must read it.
attempt('several-evaluations', function () {
  const out = [];
  for (let outer = 0; outer < 1; outer++) {
    const read = () => C3;
    const classes = [];
    for (let j = 0; j < 2; j++) {
      class Holder3 {
        value() {
          return C3;
        }
      }
      classes.push(Holder3);
    }
    class C3 {}
    const A = classes[0];
    const B = classes[1];
    out.push([new A().value() === C3, new B().value() === C3, A === B, read() === C3]);
  }
  return out;
});

// An instance created before the later class initializes reads it afterwards.
attempt('instance-before-class', function () {
  const out = [];
  for (let i = 0; i < 1; i++) {
    const read = () => C4;
    class Holder4 extends Error {
      value() {
        return C4;
      }
    }
    const early = new Holder4();
    class C4 {}
    out.push([early.value() === C4, read() === C4, new Holder4().value() === C4]);
  }
  return out;
});

// The same without a built-in parent.
attempt('instance-before-class-plain', function () {
  const out = [];
  for (let i = 0; i < 1; i++) {
    const read = () => C5;
    class Holder5 {
      value() {
        return C5;
      }
    }
    const early = new Holder5();
    class C5 {}
    out.push([early.value() === C5, read() === C5, new Holder5().value() === C5]);
  }
  return out;
});

// A generator whose parameter default names the later class: the default is
// evaluated by the generator's own call, in this iteration's scope.
attempt('generator-default', function () {
  const out = [];
  for (let i = 0; i < 1; i++) {
    function* read1(value = C6) {
      yield value;
    }
    class C6 {}
    out.push(read1().next().value === C6);
  }
  return out;
});

attempt('generator-default-twice', function () {
  const out = [];
  for (let i = 0; i < 2; i++) {
    function* read2(value = C7) {
      yield value;
    }
    class C7 {}
    out.push(read2().next().value === C7);
  }
  return out;
});

attempt('generator-default-pattern', function () {
  const out = [];
  for (let i = 0; i < 1; i++) {
    function* read3({ a = C8 } = {}) {
      yield a;
    }
    class C8 {}
    out.push(read3().next().value === C8);
  }
  return out;
});

attempt('generator-default-arrow', function () {
  const out = [];
  for (let i = 0; i < 1; i++) {
    function* read4(value = () => C9) {
      yield value;
    }
    class C9 {}
    out.push(read4().next().value() === C9);
  }
  return out;
});

// A generator that names no outer binding keeps working.
attempt('generator-default-constant', function () {
  const out = [];
  for (let i = 0; i < 1; i++) {
    function* read5(value = 1) {
      yield value;
    }
    class C10 {}
    out.push([read5().next().value, typeof C10]);
  }
  return out;
});

// The first evaluation of a plain class in a loop that runs twice.
attempt('plain-loop-first-evaluation', function () {
  const out = [];
  for (let i = 0; i < 2; i++) {
    const read = () => C11;
    class Holder11 {
      value() {
        return C11;
      }
    }
    class C11 {}
    if (i === 0) out.push([new Holder11().value() === C11, read() === C11]);
  }
  return out;
});

// A class that captures a binding is a fresh class on its only evaluation.
attempt('capturing-class-first-evaluation', function () {
  const out = [];
  for (let i = 0; i < 2; i++) {
    const read = () => C12;
    class Holder12 {
      value() {
        return C12;
      }
    }
    class C12 {
      index() {
        return i;
      }
    }
    if (i === 0) out.push([new Holder12().value() === C12, read() === C12, new C12().index()]);
  }
  return out;
});

// The declaration is initialized before the method is called, in a plain block
// rather than a loop.
attempt('block', function () {
  const out = [];
  {
    const read = () => C13;
    class Holder13 {
      value() {
        return C13;
      }
    }
    class C13 {}
    out.push([new Holder13().value() === C13, read() === C13]);
  }
  return out;
});

// Outside a loop or block nothing changes.
attempt('plain-function', function () {
  const read = () => C14;
  class Holder14 {
    value() {
      return C14;
    }
  }
  class C14 {}
  return [new Holder14().value() === C14, read() === C14];
});

// A class with static state keeps one class object per evaluation.
attempt('static-holder-loop', function () {
  const out = [];
  for (let i = 0; i < 2; i++) {
    const read = () => C15;
    class Holder15 {
      static seen = [];
      value() {
        Holder15.seen.push(1);
        return C15;
      }
    }
    Holder15.seen.push(0);
    class C15 {}
    if (i === 0) out.push([new Holder15().value() === C15, read() === C15, Holder15.seen.length]);
  }
  return out;
});

attempt('static-holder-block', function () {
  const out = [];
  {
    const read = () => C16;
    class Holder16 {
      static tag = 'h';
      value() {
        return C16;
      }
    }
    class C16 {}
    out.push([new Holder16().value() === C16, read() === C16, Holder16.tag]);
  }
  return out;
});

// The capturing class itself extends a built-in.
attempt('extends-error-holder', function () {
  const out = [];
  for (let i = 0; i < 2; i++) {
    const read = () => C17;
    class Holder17 extends Error {
      value() {
        return C17;
      }
    }
    class C17 {}
    if (i === 0) out.push([new Holder17('m').value() === C17, read() === C17]);
  }
  return out;
});

// The block never runs: nothing may read the later class's dead zone.
attempt('skipped-block', function () {
  const skip = [1].length === 2;
  if (skip) {
    const read = () => C18;
    class Holder18 {
      value() {
        return C18;
      }
    }
    class C18 {}
    return [new Holder18().value() === C18, read() === C18];
  }
  return 'skipped';
});

// A return inside the block, after the later class is declared.
attempt('return-in-block', function () {
  for (let i = 0; i < 2; i++) {
    const read = () => C19;
    class Holder19 {
      value() {
        return C19;
      }
    }
    class C19 {}
    if (i === 0) {
      return [new Holder19().value() === C19, read() === C19];
    }
  }
  return 'unreached';
});

// A class built inside a closure that runs after the later class is declared
// captures its binding, evaluated or not.
attempt('class-in-closure', function () {
  const read = () => C20;
  const make = () => {
    class Holder20 {
      value() {
        return C20;
      }
    }
    return Holder20;
  };
  class C20 extends Error {}
  const H = make();
  return [new H().value() === C20, read() === C20];
});

// A closure made while a class is defined (a computed key, the heritage, a
// static initializer) is part of the enclosing function, not of the class: it
// reads the later class's own binding. The later class is an evaluated class
// here (`extends Error`); the cases after these use other kinds.
attempt('closure-in-computed-key', function () {
  let read;
  const Holder = class {
    [((read = () => C21), 'value')]() {}
  };
  class C21 extends Error {}
  return [read() === C21, typeof Holder];
});

attempt('closure-in-heritage', function () {
  let read;
  class Holder22 extends ((read = () => C22), Object) {}
  class C22 extends Error {}
  return [read() === C22, typeof Holder22];
});

attempt('closure-in-static-initializer', function () {
  let read;
  class Holder23 {
    static seed = ((read = () => C23), 1);
  }
  class C23 extends Error {}
  return [read() === C23, Holder23.seed];
});

// The closure sits in a class DECLARATION's computed key; the later class
// captures the function's parameter, so it is a class value of its own for this
// call (and a plain class, which is the same class either way).
attempt('closure-in-declaration-computed-key-capturing', function () {
  return (function (n) {
    let read;
    class Holder29 {
      [((read = () => C29), 'value')]() {}
    }
    class C29 {
      value() {
        return n;
      }
    }
    return [read() === C29, new C29().value(), typeof Holder29];
  })(1);
});

attempt('closure-in-declaration-computed-key-plain', function () {
  let read;
  class Holder30 {
    [((read = () => C30), 'value')]() {}
  }
  class C30 {}
  return [read() === C30, typeof Holder30];
});

attempt('closure-in-declaration-heritage-and-static', function () {
  let readHeritage;
  let readStatic;
  class Holder31 extends ((readHeritage = () => C31), Object) {
    static seed = ((readStatic = () => C31), 1);
  }
  class C31 {
    value() {
      return typeof Holder31;
    }
  }
  return [readHeritage() === C31, readStatic() === C31, new C31().value()];
});

// A subclass of a class that captures a parameter is a class value of its own
// too; nothing in its own declaration says so.
attempt('subclass-of-capturing-class', function () {
  return (function (n) {
    const read = () => Derived32;
    class Base32 {
      value() {
        return n;
      }
    }
    class Derived32 extends Base32 {}
    return [read() === Derived32, new Derived32().value()];
  })(7);
});

attempt('subclass-of-capturing-class-two-levels', function () {
  return (function (n) {
    const read = () => Leaf33;
    class Base33 {
      value() {
        return n;
      }
    }
    class Middle33 extends Base33 {}
    class Leaf33 extends Middle33 {}
    return [read() === Leaf33, new Leaf33().value()];
  })(8);
});

// The same two shapes in a function declaration that the program calls once,
// from the top level: only then is its scope known to run once (the cases above
// sit in callbacks that may run repeatedly), so the later class that captures
// the parameter is a class value of its own for that single call.
function onceComputedKey(n) {
  let read;
  class Holder35 {
    [((read = () => C35), 'value')]() {}
  }
  class C35 {
    value() {
      return n;
    }
  }
  return [read() === C35, new C35().value(), typeof Holder35];
}
let onceComputedKeyResult;
try {
  onceComputedKeyResult = onceComputedKey(1);
} catch (e) {
  onceComputedKeyResult = 'threw ' + (e && e.name);
}
outcome('run-once-closure-in-declaration-computed-key-capturing', onceComputedKeyResult);

function onceSubclass(n) {
  const read = () => Derived36;
  class Base36 {
    value() {
      return n;
    }
  }
  class Derived36 extends Base36 {}
  return [read() === Derived36, new Derived36().value()];
}
let onceSubclassResult;
try {
  onceSubclassResult = onceSubclass(7);
} catch (e) {
  onceSubclassResult = 'threw ' + (e && e.name);
}
outcome('run-once-subclass-of-capturing-class', onceSubclassResult);

// A direct eval in a closure can name any binding of its scope: the string
// `"C"` is no reference the compiler can read, yet the closure must see the
// class the later declaration initialized. A constant string lowers into a
// real read of `C`; a string the program computes goes through the eval's
// bindings object, which holds a getter for every binding in scope.
attempt('eval-constant-in-closure', function () {
  const read = () => eval('C40');
  class C40 extends Error {}
  return read() === C40;
});

attempt('eval-runtime-source-in-closure', function () {
  const read = (source) => eval(source);
  class C41 extends Error {}
  return [read('C41') === C41, read('typeof C41')];
});

attempt('eval-in-nested-closure', function () {
  const read = () => () => eval('C42');
  class C42 extends Error {}
  return read()() === C42;
});

// A direct eval in a static method sits in a member body, which lowers while
// the pending later class is hidden from it. The eval's bindings object must
// not mention that binding (compiling it used to abort), and the earlier
// closure still reads the class itself.
attempt('eval-in-static-method', function () {
  const read = () => C43;
  class Holder43 {
    static evaluate(source) {
      return eval(source);
    }
  }
  class C43 {}
  return [Holder43.evaluate('1'), read() === C43];
});

attempt('eval-in-static-method-extending-class', function () {
  const read = () => C44;
  class Holder44 {
    static evaluate(source) {
      return eval(source);
    }
  }
  class C44 extends Error {}
  return [Holder44.evaluate('1'), read() === C44];
});

// A direct eval in the function body itself, outside any closure, can create
// one that outlives the call and reads a later class: the closure the eval
// makes must read the class's own binding too.
attempt('eval-in-body-creates-closure', function () {
  const read = eval('() => C45');
  class C45 extends Error {}
  return read() === C45;
});

attempt('eval-in-body-creates-closure-runtime-source', function () {
  const source = ['() =>', 'C46'].join(' ');
  const read = eval(source);
  class C46 extends Error {}
  return [read() === C46, read() === C46];
});

// That eval, and a later one, read a later class after its declaration (a
// plain class and a subclass) ...
attempt('eval-in-body-reads-class-after-declaration', function () {
  eval('1');
  class C47 extends Error {}
  class C48 {}
  return [eval('C47') === C47, eval('C48') === C48, eval('typeof C47')];
});

// ... and before it, where the dead zone throws.
attempt('eval-in-body-reads-class-before-declaration', function () {
  let result;
  try {
    eval('C49');
    result = 'no throw';
  } catch (e) {
    result = e.name;
  }
  class C49 extends Error {}
  return [result, eval('C49') === C49];
});

// A plain class read by an earlier closure, in a loop: each evaluation is its
// own class.
attempt('plain-loop-every-evaluation', function () {
  const out = [];
  for (let i = 0; i < 3; i++) {
    const read = () => C34;
    class C34 {
      who() {
        return i;
      }
    }
    out.push([read() === C34, new (read())().who()]);
  }
  return out;
});

// A closure created inside a method reads the later class too.
attempt('closure-escaping-a-method', function () {
  for (let i = 0; i < 1; i++) {
    const read = () => C24;
    class Holder24 {
      make() {
        return () => C24;
      }
    }
    class C24 {}
    return [new Holder24().make()() === C24, read() === C24];
  }
});

// The class sits in a catch clause: still the same function as the later class.
attempt('catch-clause', function () {
  const out = [];
  for (let i = 0; i < 1; i++) {
    try {
      throw 0;
    } catch (e) {
      const read = () => C25;
      class Holder25 {
        value() {
          return C25;
        }
      }
      class C25 {}
      const H = Holder25;
      out.push([new H().value() === C25, read() === C25]);
    }
  }
  return out;
});

attempt('catch-clause-block', function () {
  const out = [];
  for (let i = 0; i < 1; i++) {
    try {
      throw 0;
    } catch (e) {
      {
        const read = () => C26;
        class Holder26 {
          value() {
            return C26;
          }
        }
        class C26 {}
        out.push([new Holder26().value() === C26, read() === C26]);
      }
    }
  }
  return out;
});

// Two later classes named alike: an inner block's `C` shadows the loop
// body's, and a module-level one sits behind both. Each reader sees its own.
class C27 {}
attempt('shadowed-later-class', function () {
  const out = [];
  for (let i = 0; i < 1; i++) {
    const outerRead = () => C27;
    {
      const innerRead = () => C27;
      class Holder27 {
        value() {
          return C27;
        }
      }
      class C27 {}
      const H = Holder27;
      out.push([new H().value() === C27, innerRead() === C27]);
    }
    class C27 {}
    out.push(outerRead() === C27);
  }
  return out;
});

// The same name pending in the enclosing function and in a closure of it.
attempt('shadowed-later-class-in-closure', function () {
  const outerRead = () => C28;
  const make = () => {
    const innerRead = () => C28;
    class Holder28 {
      value() {
        return C28;
      }
    }
    class C28 {}
    return [new Holder28().value() === C28, innerRead() === C28];
  };
  class C28 {}
  return [make(), outerRead() === C28];
});

// `using` wraps the statements after its resource in a `try`, so the later
// class is declared inside the wrapper while the earlier closure is not. The
// loop body is an async function's, where the binding lives in a per-iteration
// cell.
async function attemptAsync(label, fn) {
  try {
    outcome(label, await fn());
  } catch (e) {
    outcome(label, 'threw ' + (e && e.name));
  }
}

async function usingCases() {
  await attemptAsync('async-using-loop', async function () {
    const out = [];
    for (let i = 0; i < 2; i++) {
      using empty = null;
      const read = () => C35;
      class C35 extends Error {}
      await 0;
      out.push(read() === C35);
    }
    return out;
  });

  await attemptAsync('async-using-loop-capturing', async function () {
    const out = [];
    for (let i = 0; i < 2; i++) {
      using empty = null;
      const read = () => C36;
      class C36 {
        who() {
          return i;
        }
      }
      await 0;
      out.push([read() === C36, new C36().who()]);
    }
    return out;
  });

  await attemptAsync('async-await-using-loop', async function () {
    const out = [];
    for (let i = 0; i < 2; i++) {
      await using empty = null;
      const read = () => C37;
      class C37 extends Error {}
      await 0;
      out.push(read() === C37);
    }
    return out;
  });
}
usingCases();
