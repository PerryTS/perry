// Issue #5952 — a mixin factory that DIRECTLY returns a dynamic-parent class
// expression must still bind its result as a value.
//
// `function Mixin(B) { return class extends B {…} }` + `const M = Mixin(Base)`
// takes a fast path in HIR lowering that synthesizes a real class under the
// binding name `M`. That path used to synthesize the class and stop, never
// emitting the value binding — so `new M()` and `instanceof M` worked (both
// key off the class NAME) while `typeof M` / `M.name` / `M === M` read an
// unassigned local and saw `undefined`.
//
// Covers the neighbourhood: direct return, a NAMED returned class expression,
// return-via-const, return-via-intermediate, two mixins composed in one
// expression, a mixin over another mixin's result, and a static-parent
// captured-param factory (the class-factory specialization pass's own win,
// which must survive).

class Base {
  value = "base value";
  hello() {
    return "base hello";
  }
}

// (1) the reported shape: direct return of `class extends <param>`
function Greetable(B: any) {
  return class extends B {
    greet() {
      return "hi";
    }
  };
}

// (2) same, but the returned class expression has its own name
function Named(B: any) {
  return class Marker extends B {
    who() {
      return "marker";
    }
  };
}

// (3) bound to a const inside the factory, then returned
function ViaConst(B: any) {
  const K = class extends B {
    greet() {
      return "const";
    }
  };
  return K;
}

// (4) bounced through an intermediate variable
function ViaTemp(B: any) {
  const K = class extends B {
    greet() {
      return "temp";
    }
  };
  const K2 = K;
  return K2;
}

// (5) a second mixin, for composition
function Serializable(B: any) {
  return class extends B {
    ser() {
      return "ser";
    }
  };
}

// (6) static-parent factory capturing a param — specialized per call site by
// `specialize_captured_class_factories`; the capture must stay baked in.
function makeTagged(tag: string) {
  class Inner {
    readonly _tag = tag;
    who() {
      return "tagged:" + this._tag;
    }
  }
  return Inner;
}

// --- (1) direct return -------------------------------------------------
const M = Greetable(Base);
console.log("direct typeof:", typeof M);
console.log("direct name:", JSON.stringify(M.name));
console.log("direct method:", new M().greet());
console.log("direct inherited method:", (new M() as any).hello());
console.log("direct inherited field:", (new M() as any).value);
console.log("direct instanceof self:", new M() instanceof M);
console.log("direct instanceof base:", new M() instanceof Base);
console.log("direct constructor:", (new M() as any).constructor === M);
console.log("direct prototype:", Object.getPrototypeOf(new M()) === M.prototype);
console.log("direct identity:", (M as any) !== (Base as any));

// --- (2) named class expression ----------------------------------------
const N = Named(Base);
console.log("named typeof:", typeof N);
console.log("named name:", JSON.stringify(N.name));
console.log("named method:", new N().who());
console.log("named instanceof base:", new N() instanceof Base);

// --- (3) returned via a const binding -----------------------------------
const C = ViaConst(Base);
console.log("viaconst typeof:", typeof C);
console.log("viaconst method:", new C().greet());
console.log("viaconst inherited method:", (new C() as any).hello());
console.log("viaconst instanceof base:", new C() instanceof Base);

// --- (4) returned via an intermediate variable ---------------------------
const T = ViaTemp(Base);
console.log("viatemp typeof:", typeof T);
console.log("viatemp method:", new T().greet());
console.log("viatemp instanceof base:", new T() instanceof Base);

// --- (5) two mixins composed in one expression ---------------------------
const Composed = Serializable(Greetable(Base) as any);
console.log("composed typeof:", typeof Composed);
console.log("composed outer:", new Composed().ser());
console.log("composed inner:", (new Composed() as any).greet());
console.log("composed instanceof base:", new Composed() instanceof Base);

// --- (6) a mixin whose parent is itself a mixin result --------------------
const Mid = Greetable(Base);
const Top = Serializable(Mid as any);
console.log("chained typeof mid:", typeof Mid);
console.log("chained typeof top:", typeof Top);
console.log("chained outer:", new Top().ser());
console.log("chained instanceof base:", new Top() instanceof Base);
console.log("chained identity:", (Mid as any) !== (Top as any));

// --- (7) static-parent captured-param factory -----------------------------
const Tagged = makeTagged("S");
console.log("factory typeof:", typeof Tagged);
console.log("factory name:", JSON.stringify(Tagged.name));
console.log("factory method:", new Tagged().who());
console.log("factory captured field:", new Tagged()._tag);
