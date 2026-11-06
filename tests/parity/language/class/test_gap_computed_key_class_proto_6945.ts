// #6945: a computed object-key write onto a class prototype (or class
// constructor) must be visible to:
//   - the computed read with the same key object (ToPropertyKey),
//   - the plain string-key read,
//   - instance inheritance through the prototype chain.
// Pre-fix, the write landed somewhere the dotted `C.prototype.name` read
// found, but `C.prototype[k]` and `(new C()).name` returned undefined because
// `js_dyn_index_get` treated object indices as floats (`format!("{}", f64)`)
// instead of running ToPropertyKey / user `toString`.

class C {
  m(): number {
    return 1;
  }
}
const k: any = {
  toString(): string {
    return "protoKey";
  },
};
(C.prototype as any)[k] = { tag: 6 };
console.log("computed via instance:", JSON.stringify((new C() as any).protoKey));
console.log("computed direct:", JSON.stringify((C.prototype as any).protoKey));
console.log("computed via key obj:", JSON.stringify((C.prototype as any)[k]));

const plain: any = {};
plain[k] = { tag: 7 };
console.log("plain:", JSON.stringify(plain.protoKey));
console.log("plain via key obj:", JSON.stringify(plain[k]));

// class-constructor (static) side: same ToPropertyKey obligation
class D {
  static s(): number {
    return 1;
  }
}
const ks: any = {
  toString(): string {
    return "statKey";
  },
};
(D as any)[ks] = { tag: 6 };
console.log("static via name:", JSON.stringify((D as any).statKey));
console.log("static via key obj:", JSON.stringify((D as any)[ks]));

// coercion is observable even when the property is absent
let calls = 0;
const absent: any = {
  toString(): string {
    calls++;
    return "nope";
  },
};
console.log("absent via key obj:", JSON.stringify((C.prototype as any)[absent]));
console.log("absent coercion count:", calls);

// boolean / null keys also go through ToPropertyKey
const mixed: any = {};
mixed[true as any] = "t";
mixed[null as any] = "n";
console.log("bool key:", mixed["true"]);
console.log("null key:", mixed["null"]);
console.log("bool via true:", mixed[true as any]);

// @@toPrimitive returning a Symbol must use the symbol store (get + set),
// not stringify the Symbol (get-side ToPropertyKey parity on set — #7134 CR).
const sym = Symbol("viaPrim");
const viaSym: any = {
  [Symbol.toPrimitive](_hint: string): symbol {
    return sym;
  },
};
const holder: any = {};
holder[viaSym] = { tag: 9 };
console.log("viaPrim set+get:", JSON.stringify(holder[viaSym]));
console.log("viaPrim symbol key:", JSON.stringify(holder[sym]));
