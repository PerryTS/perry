// A function's own keys after delete and re-add: the key is present again,
// last in order, with its new attributes (the deleted marker is cleared even
// when the key's spelling was never a program atom).
function F() {}
(F as any).p = 1;
Object.defineProperty(F, "x", { value: 1, enumerable: false, configurable: true });
delete (F as any).x;
(F as any).x = 3;
console.log(Object.keys(F).join(","), Object.getOwnPropertyNames(F).filter((k) => k !== "prototype").join(","));
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(F, "x")));

class C {
  static f = 1;
}
Object.defineProperty(C, "x", { value: 1, writable: false, enumerable: false, configurable: true });
delete (C as any).x;
console.log(Object.keys(C).join(","), JSON.stringify(Object.getOwnPropertyDescriptor(C, "length")));
(C as any).x = 3;
console.log(Object.keys(C).join(","), Object.getOwnPropertyNames(C).join(","));

// A runtime-built result object: its first delete of a key spelled by the
// runtime's own layout removes it.
const it = [4, 5][Symbol.iterator]();
const r: any = it.next();
const k = "do" + "ne";
delete r[k];
console.log(Object.keys(r).join(","), JSON.stringify(r), k in r);
