// Derived constructors that run receiver-free statements before super():
// a guard that throws, a local computed from the arguments, a branch.
class Base {
  annotations: any;
  context: any;
  constructor(annotations: any, context: any) {
    this.annotations = annotations;
    this.context = context;
  }
  describe(): string {
    return Object.keys(this).join(",");
  }
}
class Suspend extends Base {
  _tag = "Suspend";
  thunk: any;
  constructor(thunk: () => number, annotations?: any, checks?: any, context?: any) {
    if (checks !== undefined) {
      throw new Error("Cannot add checks to Suspend");
    }
    super(annotations, context);
    this.thunk = thunk;
  }
}
let hooked = 0;
class Guarded extends Base {
  extra: number;
  constructor(n: number) {
    const doubled = n * 2;
    if (doubled > 10) {
      hooked++;
    } else {
      // Code before super() may change the chain the stores meet.
      Object.defineProperty(Base.prototype, "extra", {
        configurable: true,
        set(v: number) {
          hooked += v;
        },
        get() {
          return -1;
        },
      });
    }
    super(doubled, null);
    this.extra = doubled;
  }
}
class Arrow extends Base {
  tag: string;
  constructor(s: string) {
    const make = () => s + "!";
    super(make(), s);
    this.tag = s;
  }
}
const out: string[] = [];
for (let i = 0; i < 3; i++) {
  const s = new Suspend(() => i, { a: i });
  out.push(s.describe() + ":" + s.thunk() + ":" + s._tag);
}
try {
  new Suspend(() => 0, undefined, [1]);
} catch (e: any) {
  out.push(e.message);
}
const g1 = new Guarded(10);
out.push(g1.describe() + ":" + g1.extra + ":" + hooked);
const g2 = new Guarded(2);
out.push(g2.describe() + ":" + g2.extra + ":" + hooked + ":" + Object.prototype.hasOwnProperty.call(g2, "extra"));
delete (Base.prototype as any).extra;
const g3 = new Guarded(20);
out.push(g3.describe() + ":" + g3.extra + ":" + hooked);
const a = new Arrow("x");
out.push(a.describe() + ":" + a.annotations + ":" + a.tag);
console.log(out.join("\n"));
