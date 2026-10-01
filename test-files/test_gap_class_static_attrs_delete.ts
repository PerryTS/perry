// Class static attributes live with the keys of the class function object.
class C {
  static f = 1;
  static g() { return 2; }
}
const d = (k: string) => JSON.stringify(Object.getOwnPropertyDescriptor(C, k));
console.log(d("f"), d("name"), d("length"));
Object.defineProperty(C, "x", { value: 1, writable: false, enumerable: false, configurable: true });
console.log(d("x"), Object.keys(C).join(","));
try { (C as any).x = 2; } catch (e) { console.log("threw x"); }
console.log((C as any).x);
delete (C as any).x;
console.log(d("x"), "x" in C);
(C as any).x = 3;
console.log(d("x"), (C as any).x, Object.keys(C).join(","));
Object.defineProperty(C, "f", { enumerable: false });
console.log(d("f"), Object.keys(C).join(","));
delete (C as any).f;
(C as any).f = 4;
console.log(d("f"), Object.keys(C).join(","));
Object.defineProperty(C, "name", { value: "K" });
console.log(d("name"), C.name);
delete (C as any).name;
console.log(d("name"), typeof C.name);
try { (C as any).name = "Z"; } catch (e) { console.log("threw name"); }
console.log(d("name"), C.name);
class D { static name = "field"; }
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(D, "name")));
(D as any).name = "w";
console.log(D.name);
Object.freeze(C);
console.log(Object.isFrozen(C), d("f"));
try { (C as any).f = 9; } catch (e) { console.log("threw f"); }
console.log((C as any).f);
