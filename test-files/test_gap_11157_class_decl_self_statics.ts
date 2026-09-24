// #11157: a class DECLARATION in a function body (a CommonJS module body is
// one) that is evaluated per evaluation — its heritage is a runtime value, or
// it has private elements — must see THIS evaluation's class object when its
// members name the class. bson's ObjectId is the real shape: its base class
// reads a module-level const, so `class ObjectId extends BSONValue` inside
// bson.cjs is per-evaluation, and `ObjectId.index = (ObjectId.index + 1)` in a
// static method read and wrote the shared template, so every id was the same.

function bsonShape(): any {
  const MAJOR = 7;
  class Base {
    get version() {
      return MAJOR;
    }
  }
  class Oid extends Base {
    static index = 0;
    static PROCESS_UNIQUE: number[] | null = null;
    static resetState = () => {
      this.index = 100;
      this.PROCESS_UNIQUE = null;
    };
    static {
      this.resetState();
      console.log("static block: this === Oid", this === Oid, "index", Oid.index);
    }
    static cacheHexString: boolean | undefined;
    inc: number;
    pu: number[];
    constructor() {
      super();
      this.inc = Oid.getInc();
      this.pu = (Oid.PROCESS_UNIQUE ??= [1, 2, 3, 4, 5]);
    }
    static getInc() {
      return (Oid.index = (Oid.index + 1) % 0x1000000);
    }
    static createPk() {
      return new Oid();
    }
    static bumpTwice() {
      Oid.index += 2;
      Oid.index++;
      return Oid.index;
    }
    sameClass() {
      return this.constructor === Oid;
    }
  }
  return Oid;
}

const factories: any[] = [bsonShape];
const Oid = factories[0]();
const a = new Oid();
const b = new Oid();
const c = Oid.createPk();
console.log("counters", a.inc, b.inc, c.inc);
console.log("distinct", new Set([a.inc, b.inc, c.inc]).size);
console.log("PROCESS_UNIQUE shared", a.pu === b.pu && b.pu === c.pu, "length", a.pu.length);
console.log("static index", Oid.index, "cacheHexString", Oid.cacheHexString);
console.log("constructor === Oid", a.sameClass(), "version", a.version);
console.log("compound", Oid.bumpTwice(), Oid.index);
Oid.index = 500;
Oid.index += 1;
console.log("outside write", Oid.index, new Oid().inc);
Oid.resetState();
console.log("after resetState", Oid.index, Oid.PROCESS_UNIQUE);

// Two evaluations keep their own statics.
const Oid2 = factories[0]();
const d = new Oid2();
console.log("second evaluation", Oid2 !== Oid, d.inc, Oid.index, Oid2.index);

// A nested arrow in a static initializer still sees the evaluation.
function nestedArrow(): any {
  const tag = "t";
  class Base {
    t() {
      return tag;
    }
  }
  class N extends Base {
    static label = "n";
    static self = () => () => this;
    static read() {
      return N.label;
    }
  }
  return N;
}
const N = [nestedArrow][0]();
console.log("nested arrow this", N.self()() === N, N.read());

// Private elements make a function-body class per-evaluation too.
function privateShape(): any {
  class P {
    #x = 1;
    static n = 0;
    inc: number;
    constructor() {
      this.inc = P.bump();
    }
    static bump() {
      return (P.n = P.n + 1);
    }
    get x() {
      return this.#x;
    }
  }
  return P;
}
const P = [privateShape][0]();
const p1 = new P();
const p2 = new P();
console.log("private", p1.inc, p2.inc, P.n, p1.x);
