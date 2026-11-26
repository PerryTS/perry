// A plain-named static method that follows a computed instance member is
// registered in source order; it is still its declaration, with one
// function object body. A proven static call on it must find that function
// (check mode: a second JsFunctionInfo read as "Replaced", exit 86).
const K = "k";

class G {
  [K]() {
    return "k";
  }
  static viaLocal() {
    return 1;
  }
  static viaThis() {
    return this.viaLocal() + 1;
  }
}

console.log(G.viaLocal(), G.viaThis(), new G().k());
const f = G.viaLocal;
console.log(f(), f.name, f.length, f === G.viaLocal);
console.log(Object.getOwnPropertyNames(G).join(","));

class H extends G {}
console.log(H.viaLocal(), H.viaThis(), H.viaLocal === G.viaLocal);

const E = class {
  [K]() {
    return 2;
  }
  static after(x: number) {
    return x * 3;
  }
};
console.log(E.after(4), E.after.name, E.after.length);

// A later redefinition is still seen as one.
class R {
  [K]() {
    return 0;
  }
  static m() {
    return "declared";
  }
}
console.log(R.m());
(R as any).m = () => "replaced";
console.log(R.m());
