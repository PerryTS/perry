// A closure created before a per-evaluation class declaration (heritage is the
// native Error) reads the class through the declaration's binding. An object
// literal's method, getter and setter are closures too: a parameter default or
// a destructuring default in their parameter list names the class when the
// method runs, and must resolve to the class of its own evaluation, not to the
// shared template. Every line is compared with node. Each case runs on its own
// so that one failure does not hide the others (a case that throws prints
// `threw`).

function attempt(label, fn) {
  try {
    console.log(label, JSON.stringify(fn()));
  } catch (e) {
    console.log(label, 'threw', e && e.name);
  }
}

function make(tag) {
  const obj = {
    tag,
    read(ctor = C) {
      return ctor;
    },
    destructured({ ctor = C } = {}) {
      return ctor;
    },
    set seed(value = C) {
      this.stored = value;
    },
    get klass() {
      return C;
    },
    *generate(ctor = C) {
      yield ctor;
    },
  };
  class C extends Error {}
  return { obj, C };
}

const a = make('a');
const b = make('b');
attempt('default', () => [
  a.obj.read() === a.C,
  b.obj.read() === b.C,
  a.obj.read() !== b.C,
]);
attempt('destructured', () => [
  a.obj.destructured() === a.C,
  b.obj.destructured() === b.C,
  a.obj.destructured() !== b.C,
]);
attempt('setter', () => {
  a.obj.seed = undefined;
  b.obj.seed = undefined;
  return [a.obj.stored === a.C, b.obj.stored === b.C];
});
attempt('generator', () => [
  a.obj.generate().next().value === a.C,
  b.obj.generate().next().value === b.C,
]);
attempt('getter', () => [a.obj.klass === a.C, b.obj.klass === b.C]);
attempt('instance', () => {
  const error = new (a.obj.read())('boom');
  return [error instanceof a.C, error instanceof b.C, error.message];
});

// A default evaluated before the declaration ran is in its dead zone.
function early() {
  const obj = {
    read(ctor = C) {
      return ctor;
    },
  };
  let before;
  try {
    obj.read();
    before = 'no error';
  } catch (e) {
    before = e.name;
  }
  class C extends Error {}
  return [before, obj.read() === C];
}
attempt('dead-zone', () => [early(), early()]);

// The same default over a `let`: the scan that finds a closure's captures is
// shared by every lexical declaration, so a method's parameter list counts for
// all of them.
function earlyLet() {
  const obj = {
    read(value = x) {
      return value;
    },
  };
  let before;
  try {
    obj.read();
    before = 'no error';
  } catch (e) {
    before = e.name;
  }
  let x = 1;
  return [before, obj.read()];
}
attempt('dead-zone-let', () => [earlyLet(), earlyLet()]);
