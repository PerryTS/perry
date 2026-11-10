// #7512 / #7515: the dead-default-field-init elision must not change how many
// times an accessor runs.
//
// A setter installed on `C.prototype` AFTER compilation is invisible to the
// compile-time `class.setters` check that guards the elision, so this pins the
// observable behaviour directly rather than through that check.
//
// The spec answer is ZERO setter calls: a class field declaration (`v;`) is a
// CreateDataProperty — a DEFINE, not a [[Set]] — so it never consults an
// inherited accessor, and it installs an OWN data property that the
// constructor's `this.v = v` then writes directly. Eliding the default write
// therefore removes nothing observable. (The "field init is a [[Set]]" reading
// is the legacy `useDefineForClassFields: false` behaviour, which is not what
// this compiler or Node implements.)

let calls = 0;
let seen = "";

class C {
  v: number;
  w: number;
  constructor(v: number, w: number) {
    this.v = v;
    this.w = w;
  }
}

Object.defineProperty(C.prototype, "v", {
  configurable: true,
  set(x: any) {
    calls = calls + 1;
    seen = seen + "[" + String(x) + "]";
  },
  get(): any {
    return 999;
  },
});

const c1 = new C(1, 2);
console.log("setter_calls", calls);
console.log("setter_args", seen);
console.log("read_v", c1.v);
console.log("read_w", c1.w);

// Per-construction and stable across instances.
const c2 = new C(3, 4);
console.log("setter_calls_after_2", calls);
console.log("read_v2", c2.v, "read_w2", c2.w);

// A setter on a SEPARATE prototype the class does not declare, reached through
// the same post-compilation route, on a field the prologue assigns second.
let wCalls = 0;

class D {
  a: number;
  b: number;
  constructor(a: number, b: number) {
    this.a = a;
    this.b = b;
  }
}

Object.defineProperty(D.prototype, "b", {
  configurable: true,
  set(x: any) {
    wCalls = wCalls + 1;
  },
  get(): any {
    return -1;
  },
});

const d = new D(8, 9);
console.log("d_setter_calls", wCalls);
console.log("d", d.a, d.b);

// A field the prologue does NOT assign still reads `undefined`.
class E {
  a: number;
  b: number;
  c: number;
  constructor(a: number, b: number) {
    this.a = a;
    this.b = b;
  }
}

const e = new E(5, 6);
console.log("e", e.a, e.b, String(e.c), e.c === undefined, JSON.stringify(e));
