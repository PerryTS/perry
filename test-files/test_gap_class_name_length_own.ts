// A class constructor's `length` and `name` are own data properties of its
// function object: { writable: false, enumerable: false, configurable: true },
// created before any static. Static fields, methods, accessors and
// defineProperty of the same name replace them; delete removes them.
function show(label: string, v: any): void {
  console.log(label, JSON.stringify(v));
}
function desc(o: any, k: string): string {
  const d = Object.getOwnPropertyDescriptor(o, k);
  if (!d) return "none";
  return `${typeof d.value === "function" ? "fn" : JSON.stringify(d.value)} w=${d.writable} e=${d.enumerable} c=${d.configurable}`;
}

class A {
  static s = 1;
  constructor(a: number, b: number) {}
}
show("A.name", A.name);
show("A.length", A.length);
show("names", Object.getOwnPropertyNames(A));
show("keys", Object.keys(A));
show("entries", Object.entries(A));
show("spread", { ...(A as any) });
show("assign", Object.assign({}, A));
console.log("desc name", desc(A, "name"));
console.log("desc length", desc(A, "length"));
console.log("own", A.hasOwnProperty("name"), A.hasOwnProperty("length"), "name" in A);
const forin: string[] = [];
for (const k in A) forin.push(k);
show("forin", forin);

const anyA: any = A;
show("dyn name", anyA.name);
show("dyn length", anyA["length"]);
show("ctor name", new A(1, 2).constructor.name);

class B extends A {}
show("B.name", B.name);
show("B.length", B.length);
show("B names", Object.getOwnPropertyNames(B));

class F {
  static name = "Field";
}
show("F.name", F.name);
console.log("F desc", desc(F, "name"));
show("F keys", Object.keys(F));
show("F names", Object.getOwnPropertyNames(F));

class M {
  static name() {
    return "method";
  }
}
show("M.name()", M.name());
console.log("M desc", desc(M, "name"));

class G {
  static get name() {
    return "getter";
  }
}
show("G.name", G.name);

class D {}
Object.defineProperty(D, "name", { value: "Defined" });
show("D.name", D.name);
console.log("D desc", desc(D, "name"));
show("D keys", Object.keys(D));

class E {}
show("delete", delete (E as any).name);
show("E.name after delete", E.name);
show("E own", E.hasOwnProperty("name"));
show("E names", Object.getOwnPropertyNames(E));

class L {
  constructor(a: number, b = 2, ...rest: number[]) {}
}
show("L.length", L.length);
delete (L as any).length;
show("L.length after delete", L.length);


const Named = class {};
show("named expr", Named.name);

class P {
  static x = 5;
}
console.log(P);
