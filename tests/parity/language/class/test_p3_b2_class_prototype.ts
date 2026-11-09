// P3 B2 controls (DESIGN-p3 rev 3.1 §3): `C.prototype` is the class's
// prototype object everywhere a program can see it, never a word that reads
// as a Number. Own keys are exactly the declaration's (no key added or
// dropped by materializing the prototype).

function describe(p: any): string {
  return [typeof p, p === null, Number(p), String(p), p + 1, Object.getOwnPropertyNames(p).join("|")].join(",");
}

// Read before any `new`, then after.
class A { m(): number { return 1; } get g(): number { return 2; } static s(): number { return 3; } }
const before = A.prototype;
console.log("A before", describe(before), before === A.prototype);
const a = new A();
console.log("A after", Object.getPrototypeOf(a) === A.prototype, Object.getPrototypeOf(a) === before, a instanceof A);
console.log("A keys", Object.keys(A.prototype).length, Object.getOwnPropertyNames(A.prototype).join("|"),
  Object.getOwnPropertyDescriptor(A.prototype, "constructor")!.enumerable, A.prototype.constructor === A);

// `new` first, then the read.
class B { x = 1; }
const b = new B();
console.log("B", Object.getPrototypeOf(b) === B.prototype, describe(B.prototype), b instanceof B);

// Static initializers see the prototype.
class C {
  static seen = this.prototype;
  static ok: boolean;
  static { C.ok = Object.getPrototypeOf(new C()) === this.prototype; }
  n(): string { return "n"; }
}
console.log("C static", C.seen === C.prototype, C.ok, typeof C.seen);

// Derived classes, class expressions, anonymous classes.
class D extends A { m(): number { return 10; } }
const E = class { e(): number { return 5; } };
const F = (() => class {})();
console.log("D", Object.getPrototypeOf(D.prototype) === A.prototype, new D() instanceof A, new D().m(), describe(D.prototype));
console.log("E F", describe(E.prototype), describe(F.prototype), new E() instanceof E, Object.getPrototypeOf(new F()) === F.prototype);

// defineProperty on the prototype, visible through instances.
Object.defineProperty(B.prototype, "tag", { value: "t", enumerable: false, configurable: true });
console.log("define", (b as any).tag, (new B() as any).tag, Object.getOwnPropertyNames(B.prototype).join("|"));

// The prototype as a Map key, an object key and an index.
const m = new Map<any, string>([[A.prototype, "a"], [B.prototype, "b"]]);
const o: any = {};
o[A.prototype as any] = 1;
const arr: any[] = [];
arr[C.prototype as any] = 2;
console.log("keys", m.get(A.prototype), m.get(B.prototype), m.get(Object.getPrototypeOf(a)), Object.keys(o).join(","),
  arr.length, Object.keys(arr).join(","), new Set([A.prototype, A.prototype]).size);

// Reflect.construct with a class newTarget takes its prototype.
const viaReflect = Reflect.construct(A, [], B);
console.log("reflect", Object.getPrototypeOf(viaReflect) === B.prototype, viaReflect instanceof B);
