// #12958: a class declared inside a function that extends an Error-family
// builtin is a per-evaluation class whose `super()` reaches the native parent
// through the dynamic construct site. Its instance must stay the derived
// object, and a method call on it must find the class's prototype methods
// rather than answer `{}`.

function direct() {
  class B extends Error { who() { return 'base'; } }
  return new B('m').who();
}
console.log(direct());

function explicitCtor() {
  class B extends Error {
    constructor(m: string) { super(m); }
    who() { return 'base:' + this.message; }
  }
  return new B('m').who();
}
console.log(explicitCtor());

function viaLocal() {
  class B extends RangeError { who() { return 'range'; } }
  const b = new B('r');
  return b.who();
}
console.log(viaLocal());

function escapes(tag: string) {
  class Tagged extends TypeError {
    code = 'E_' + tag;
    who() { return this.describe() + '/' + this.code; }
    describe() { return this.name + ':' + this.message; }
  }
  return new Tagged('boom ' + tag);
}
const t = escapes('x');
console.log(t.who(), t instanceof TypeError, t instanceof Error, Object.keys(t).join(','));

function escapesClass() {
  class B extends Error { who() { return 'from outside'; } }
  return B;
}
const C = escapesClass();
const c = new C('o');
console.log(c.who(), c.message, c instanceof C);

function missingMethod() {
  class B extends Error { who() { return 'base'; } }
  const b: any = new B('m');
  try {
    b.nope();
  } catch (err) {
    console.log('missing', (err as Error).constructor.name);
  }
}
missingMethod();
