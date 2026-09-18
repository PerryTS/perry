// #10485 / #10489: class members nested in a function scope (a function body,
// or a CommonJS module body) must share the enclosing bindings they capture,
// exactly like closures do: they see writes made after the class was declared,
// and their own writes are visible outside and to every other member/instance.
//
// Perry lifts class members out of their function and hands them captures as
// value snapshots; a mutable capture is shared through a one-element cell
// (#5951). A `var` is declared twice in HIR (the body-entry slot, then the
// declaration statement), which the cell rewrite read as two bindings and
// skipped, so every `var` capture stayed a stale copy. Separately, two
// functions that each bound `const K = class {…}` constructed the FIRST
// function's class from inside the second one.
//
// Validated byte-for-byte against `node --experimental-strip-types`.
import cjs from "./_helpers/class_member_var_captures_10485.cjs";

const show = (label: string, value: unknown) => console.log(label, JSON.stringify(value));

// ── 1. TypeScript `var _a; class X {…} _a = X;` emit in a CommonJS module ──
{
  const lib = cjs as any;
  const client = lib.Client.create("a");
  show("cjs static via _a:", client.tagged());
  show("cjs extends _a:", client instanceof lib.Client);
  lib.bumpHits(10);
  show("cjs hits (member write + outer write):", [lib.Client.hits(), lib.readHits()]);
}

// ── 2. reads after a later assignment, every member kind ──
function readsAfterAssignment(param: number) {
  var withInit = 1;
  var noInit: any;
  let lexical = 1;
  class K {
    snapshot = [withInit, noInit, lexical, param];
    live = () => [withInit, noInit, lexical, param];
    static s() { return [withInit, noInit, lexical, param]; }
    i() { return [withInit, noInit, lexical, param]; }
    get g() { return [withInit, noInit, lexical, param]; }
    static get sg() { return [withInit, noInit, lexical, param]; }
    nested() { return new (class { r() { return [withInit, noInit, lexical, param]; } })().r(); }
  }
  withInit = 2; noInit = "set"; lexical = 2; param = 20;
  const k = new K();
  show("after 1st: static", K.s());
  show("after 1st: static getter", K.sg);
  show("after 1st: instance", k.i());
  show("after 1st: getter", k.g);
  show("after 1st: field init", k.snapshot);
  show("after 1st: field arrow", k.live());
  show("after 1st: nested class", k.nested());
  withInit = 3; noInit = "again"; lexical = 3; param = 30;
  show("after 2nd: static", K.s());
  show("after 2nd: old instance", [k.i(), k.snapshot, k.live()]);
  // a `var` re-declaration with an initializer writes the SAME binding
  var withInit = 4;
  show("after var redeclaration:", [K.s(), k.i()]);
}
readsAfterAssignment(10);

// a `var` declared after the class (a `let`/`const` declared after a class that
// reads it is a separate, still-open gap — it stays undefined in Perry today)
function declaredAfterClass() {
  class K {
    static v() { return late; }
  }
  var late = "late var";
  return [K.v()];
}
show("declared after class:", declaredAfterClass());

// ── 3. writes from class members ──
function ctorCounter() {
  var count = 0;
  class A { id: number; constructor() { this.id = ++count; } }
  const ids = [new A().id, new A().id, new A().id];
  return [ids, count];
}
show("ctor counter:", ctorCounter());

function staticAndInstanceWrites() {
  var total = 0;
  var log = "";
  class B {
    static add(n: number) { total += n; }
    push(s: string) { log = log + s; return log.length; }
    set value(v: number) { total = v; }
    get value() { return total; }
  }
  B.add(1); B.add(2);
  const b1 = new B(), b2 = new B();
  b1.push("x"); b2.push("y");
  const afterStatic = total;
  b1.value = 100;
  return [afterStatic, total, b2.value, log];
}
show("static/instance/setter writes:", staticAndInstanceWrites());

function fieldInitWrite() {
  var next = 1;
  class F { id = next++; }
  new F(); new F();
  return [new F().id, next];
}
show("field initializer write:", fieldInitWrite());

function postfixValue() {
  var n = 0;
  let m = 0;
  class P { a() { return n++; } b() { return m++; } }
  const p = new P();
  p.a(); p.a(); p.b(); p.b();
  return [p.a(), n, p.b(), m];
}
show("postfix result:", postfixValue());

function nestedClassWrites() {
  var made = 0;
  class Outer {
    make() { return class Inner { constructor() { made++; } }; }
  }
  const Inner = new Outer().make();
  new Inner(); new Inner();
  return made;
}
show("nested class write:", nestedClassWrites());

function fieldInitNestedClass() {
  var seen = 0;
  class Holder {
    Tracked = class { constructor() { seen++; } };
    count() { return seen; }
  }
  const h = new Holder();
  new h.Tracked(); new h.Tracked();
  return [h.count(), seen];
}
show("field-init nested class:", fieldInitNestedClass());

// a `var` in a loop body is ONE function-scoped binding
function loopVar() {
  const classes: any[] = [];
  for (let i = 0; i < 3; i++) {
    var shared = i * 10;
    class C { static get() { return shared; } }
    classes.push(C);
  }
  return classes.map((c) => c.get());
}
show("var in loop body:", loopVar());

// ── 4. emscripten FS shape: hoisted function constructs a later class expression ──
function hoistedFactory() {
  function createNode() { return new FSNode(); }
  var nextInode = 1, FSNode = class { id: number; constructor() { this.id = nextInode++; } };
  createNode(); createNode();
  return createNode().id + "/" + nextInode;
}
show("hoisted factory:", hoistedFactory());

// ── 5. same-named bindings and classes in sibling functions ──
function twinOne() { var n = 0; const K = class { static tag = "one"; constructor() { n++; } }; new K(); new K(); new K(); return [n, K.tag, new K() instanceof K]; }
function twinTwo() { var n = 0; const K = class { static tag = "two"; constructor() { n += 10; } }; new K(); new K(); new K(); return [n, K.tag, new K() instanceof K]; }
function twinDeclOne() { var n = 0; class D { constructor() { n++; } } new D(); new D(); return n; }
function twinDeclTwo() { var n = 0; class D { constructor() { n += 100; } } new D(); new D(); return n; }
function notAClass() {
  var K: any = function (this: any) { this.kind = "function"; };
  return new K().kind;
}
show("twin expr one:", twinOne());
show("twin expr two:", twinTwo());
show("twin decl:", [twinDeclOne(), twinDeclTwo()]);
show("same-named non-class local:", notAClass());

// ── 6. module top level (always worked; must keep working) ──
var _top: any;
class Top { static viaAlias() { return _top === Top; } }
_top = Top;
show("esm top-level alias:", Top.viaAlias());
