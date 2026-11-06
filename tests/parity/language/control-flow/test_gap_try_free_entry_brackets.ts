// ehfree slice 1: state a frame opens around a call (the derived-constructor
// `super()` binding scope an arrow reads, the `new.target` bridge around a
// constructor call, a private-name environment) must be closed when a throw
// abandons it — by the catching frame's landing pad, or by a pad action when
// the frame catches its own throw — and stay open when it does not.

class Base {
  x: number;
  constructor(x: number) {
    this.x = x;
  }
}

class D extends Base {
  y = 0;
  constructor(fail: boolean) {
    super(1);
    const f = () => this.x;
    if (fail) throw new Error("ctor threw");
    this.y = f() + 1;
  }
}

let built = 0;
let failed = 0;
for (let i = 0; i < 1000; i++) {
  try {
    const d = new D(i % 2 === 0);
    built += d.y;
  } catch {
    failed++;
  }
}
console.log("derived: built", built, "failed", failed);

// An arrow read before `super()` must still see the TDZ after all those
// abandoned scopes, and the binding after it.
class E extends Base {
  constructor() {
    const g = () => this;
    try {
      g();
      console.log("E: WRONG, read before super");
    } catch (e) {
      console.log("E: TDZ", (e as Error).constructor.name);
    }
    super(5);
    console.log("E: after super", g().x);
  }
}
new E();

// new.target after constructors that threw.
function plain(): boolean {
  return new.target === undefined;
}
class F {
  constructor() {
    throw new Error("F");
  }
}
let fCaught = 0;
for (let i = 0; i < 100; i++) {
  try {
    new F();
  } catch {
    fCaught++;
  }
}
console.log("new.target: caught", fCaught, "plain call sees undefined:", plain());

function Ctor(this: any) {
  this.isNew = new.target !== undefined;
}
const viaNew = new (Ctor as any)();
const viaCall: any = {};
Ctor.call(viaCall);
console.log("new.target: via new", viaNew.isNew, "via call", viaCall.isNew);

// Private methods called through throws keep their lexical environment.
class P {
  #v = 7;
  #boom(n: number): number {
    if (n % 3 === 0) throw new Error("boom");
    return this.#v + n;
  }
  run(n: number): number {
    try {
      return this.#boom(n);
    } catch {
      return -1;
    }
  }
}
const p = new P();
let acc = 0;
for (let i = 0; i < 300; i++) acc += p.run(i);
console.log("private: acc", acc);

// Static private access after caught throws in static methods.
class S {
  static #count = 0;
  static bump(fail: boolean): number {
    S.#count++;
    if (fail) throw new Error("static");
    return S.#count;
  }
}
let last = 0;
for (let i = 0; i < 200; i++) {
  try {
    last = S.bump(i % 4 === 0);
  } catch {}
}
console.log("static private: last", last);
