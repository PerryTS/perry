// A symbol-keyed write on a class runs its own or inherited `static set [S]`
// with `this` = the class written to, instead of storing a data member that
// shadows the accessor; a getter-only static symbol accessor stores nothing.
const S = Symbol("s");
let calls = 0;
class Own {
  static set [S](v: unknown) {
    calls++;
    (this as any).stored = v;
  }
}
class Deep extends Own {}
(Own as any)[S] = 1;
console.log("own set:", calls, Object.prototype.hasOwnProperty.call(Own, "stored"), (Own as any).stored);
(Deep as any)[S] = 7;
console.log("inherited set:", calls, Object.prototype.hasOwnProperty.call(Deep, "stored"), (Deep as any).stored);
console.log("Reflect.set:", Reflect.set(Deep, S, 8), calls, (Deep as any).stored);
const anyDeep: any = Deep;
anyDeep[S] = 9;
console.log("any-typed set:", calls, anyDeep.stored);
console.log("own symbols:", Object.getOwnPropertySymbols(Own).length, Object.getOwnPropertySymbols(Deep).length);
console.log("ownKeys S once:", Reflect.ownKeys(Own).filter((k) => k === S).length, Reflect.ownKeys(Deep).filter((k) => k === S).length);
console.log("in:", S in Own, S in Deep);
console.log("hasOwn:", Object.prototype.hasOwnProperty.call(Own, S), Object.prototype.hasOwnProperty.call(Deep, S));
const d = Object.getOwnPropertyDescriptor(Own, S)!;
console.log("descriptor:", typeof d.get, typeof d.set, d.enumerable, d.configurable, d.set!.name);

class G {
  static get [S]() {
    return (this as any).name;
  }
}
class G2 extends G {}
console.log("get through chain:", (G as any)[S], (G2 as any)[S]);
console.log("Reflect.set getter-only:", Reflect.set(G2, S, 9), Object.getOwnPropertySymbols(G2).length, (G2 as any)[S]);
try {
  (G2 as any)[S] = 9;
  console.log("getter-only write: no throw");
} catch (e) {
  console.log("getter-only write:", (e as Error).constructor.name);
}
function strictWrite(): void {
  "use strict";
  (G2 as any)[S] = 10;
}
try {
  strictWrite();
  console.log("strict getter-only write: no throw");
} catch (e) {
  console.log("strict getter-only write:", (e as Error).constructor.name);
}
console.log("getter-only after:", Object.getOwnPropertySymbols(G2).length, (G2 as any)[S]);

class Plain {}
(Plain as any)[S] = 3;
console.log("plain:", (Plain as any)[S], Object.getOwnPropertySymbols(Plain).length, Object.prototype.hasOwnProperty.call(Plain, S));
(Plain as any)[S] = 4;
console.log("plain again:", (Plain as any)[S], Object.getOwnPropertySymbols(Plain).length);
