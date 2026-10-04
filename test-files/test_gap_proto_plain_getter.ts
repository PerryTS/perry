// An inherited accessor whose getter is a function object (a compiled
// function body or a builtin thunk), not a compiled class getter: the read
// site's holder accessor entry calls it with the closure ABI. Each row reads
// through its own site per name, so the site primes on the first read and
// answers the rest from its entry; rows 3-5 then change what the entry proved
// and check that the next read sees it.

// A compiled class getter named `flag` makes `flag` sites emit the inline
// accessor arm; `pp`, `twice` and `gz` take the collecting slow call.
class Decl {
  k = 5;
  get flag() {
    return this.k === 5;
  }
}
const decl = new Decl();

const N = 2000;

// 1. proto_getter_plain: a defineProperty getter on a plain prototype.
{
  const readFlag = (o: any): any => o.flag;
  const P: any = {};
  Object.defineProperty(P, "flag", {
    get: function (this: any) {
      return this.k === 1;
    },
    enumerable: false,
    configurable: true,
  });
  const a: any = Object.create(P);
  a.k = 1;
  const b: any = Object.create(P);
  b.k = 2;
  let hits = 0;
  for (let i = 0; i < N; i++) {
    if (readFlag(a)) hits++;
    if (readFlag(b)) hits += 1000;
  }
  console.log("proto_getter_plain", hits, readFlag(decl));
}

// 2. A builtin thunk as the getter: %Object.prototype%'s `__proto__` getter
// installed under another name, and a builtin getter whose brand check
// throws on an ordinary receiver.
{
  const readPP = (o: any): any => o.pp;
  const protoGet = (Object.getOwnPropertyDescriptor(Object.prototype, "__proto__") as any).get;
  const P: any = { tag: "P" };
  Object.defineProperty(P, "pp", { get: protoGet, configurable: true });
  const o: any = Object.create(P);
  let same = 0;
  for (let i = 0; i < N; i++) if (readPP(o) === P) same++;
  const sizeGet = (Object.getOwnPropertyDescriptor(Map.prototype, "size") as any).get;
  const Q: any = {};
  Object.defineProperty(Q, "pp", { get: sizeGet, configurable: true });
  const q: any = Object.create(Q);
  let threw = 0;
  for (let i = 0; i < 50; i++) {
    try {
      readPP(q);
    } catch (e) {
      if (e instanceof TypeError) threw++;
    }
  }
  console.log("builtin_getter", same, threw);
}

// 3. The getter replaced through defineProperty with the SAME attributes:
// no ShapeId moves, only the lane's pair does.
{
  const readFlag = (o: any): any => o.flag;
  const readGz = (o: any): any => o.gz;
  const P: any = {};
  const attrs = { enumerable: false, configurable: true };
  Object.defineProperty(P, "flag", {
    get: function (this: any) {
      return 1;
    },
    ...attrs,
  });
  const o: any = Object.create(P);
  let sum = 0;
  for (let i = 0; i < N; i++) {
    if (i === N / 2) {
      Object.defineProperty(P, "flag", {
        get: function (this: any) {
          return 100;
        },
        ...attrs,
      });
    }
    sum += readFlag(o);
  }
  const P2: any = {};
  Object.defineProperty(P2, "gz", {
    get: function (this: any) {
      return 1;
    },
    ...attrs,
  });
  const o2: any = Object.create(P2);
  let sum2 = 0;
  for (let i = 0; i < N; i++) {
    if (i === N / 2) {
      Object.defineProperty(P2, "gz", {
        get: function (this: any) {
          return 100;
        },
        ...attrs,
      });
    }
    sum2 += readGz(o2);
  }
  console.log("replaced_getter", sum, sum2);
}

// 4. The getter deleted from the prototype.
{
  const readFlag = (o: any): any => o.flag;
  const P: any = {};
  Object.defineProperty(P, "flag", {
    get: function (this: any) {
      return 7;
    },
    configurable: true,
  });
  const o: any = Object.create(P);
  let sum = 0;
  let undef = 0;
  for (let i = 0; i < N; i++) {
    if (i === N / 2) delete P.flag;
    const v = readFlag(o);
    if (v === undefined) undef++;
    else sum += v;
  }
  console.log("deleted_getter", sum, undef);
}

// 5. setPrototypeOf on the receiver: same own keys, another prototype.
{
  const readFlag = (o: any): any => o.flag;
  const P1: any = {};
  Object.defineProperty(P1, "flag", {
    get: function (this: any) {
      return this.k;
    },
    configurable: true,
  });
  const P2: any = {};
  Object.defineProperty(P2, "flag", {
    get: function (this: any) {
      return 10 * this.k;
    },
    configurable: true,
  });
  const o: any = Object.create(P1);
  o.k = 3;
  let sum = 0;
  for (let i = 0; i < N; i++) {
    if (i === N / 2) Object.setPrototypeOf(o, P2);
    sum += readFlag(o);
  }
  console.log("set_prototype_of", sum);
}

// 6. The getter forms `[[Get]]` calls: an object-literal accessor, a
// method-shorthand descriptor getter, an arrow (lexical `this`), a getter
// that throws, and a getter on a class prototype added by defineProperty.
{
  const readTwice = (o: any): any => o.twice;
  const readGz = (o: any): any => o.gz;
  const lit: any = {
    k: 0,
    get twice() {
      return this.k * 2;
    },
  };
  const a: any = Object.create(lit);
  a.k = 4;
  const D: any = {};
  Object.defineProperty(D, "twice", {
    get() {
      return (this as any).k * 3;
    },
    configurable: true,
  });
  const d: any = Object.create(D);
  d.k = 4;
  const outer = { k: 100 };
  const A: any = {};
  const arrowGet = (function (this: any) {
    return () => this.k;
  }).call(outer);
  Object.defineProperty(A, "twice", { get: arrowGet, configurable: true });
  const ar: any = Object.create(A);
  ar.k = 1;
  let s = 0;
  for (let i = 0; i < N; i++) s += readTwice(a);
  let t = 0;
  for (let i = 0; i < N; i++) t += readTwice(d);
  let u = 0;
  for (let i = 0; i < N; i++) u += readTwice(ar);
  const T: any = {};
  Object.defineProperty(T, "gz", {
    get: function (this: any) {
      if (this.k > 2) throw new RangeError("k " + this.k);
      return this.k;
    },
    configurable: true,
  });
  const tr: any = Object.create(T);
  let ok = 0;
  let caught = 0;
  for (let i = 0; i < 6; i++) {
    tr.k = i;
    try {
      ok += readGz(tr);
    } catch (e) {
      if (e instanceof RangeError) caught++;
    }
  }
  class C {
    k = 9;
  }
  Object.defineProperty(C.prototype, "gz", {
    get: function (this: any) {
      return this.k + 1;
    },
    configurable: true,
  });
  const c = new C();
  let v = 0;
  for (let i = 0; i < N; i++) v += readGz(c);
  console.log("getter_forms", s, t, u, ok, caught, v);
}

// 7. A getter on %Object.prototype% read through a plain literal.
{
  const readGz = (o: any): any => o.gz;
  Object.defineProperty(Object.prototype, "gz", {
    get: function (this: any) {
      return typeof this.k === "number" ? this.k : -1;
    },
    configurable: true,
  });
  const o: any = { k: 2 };
  let sum = 0;
  for (let i = 0; i < N; i++) sum += readGz(o);
  delete (Object.prototype as any).gz;
  console.log("object_prototype_getter", sum, readGz(o));
}

// 8. A getter defined after the prototype's birth, past its inline slots
// (the lane lives in spill storage), then replaced with the same attributes.
{
  const readFlag = (o: any): any => o.flag;
  const f = function (this: any, s: any) {
    return s.length > 3;
  };
  const P: any = { m: f, k0: 0, k1: 1, k2: 2, k3: 3, k4: 4, k5: 5, k6: 6 };
  Object.defineProperty(P, "flag", {
    get: function (this: any) {
      return this.k;
    },
    configurable: true,
  });
  const o: any = Object.create(P);
  o.k = 2;
  let sum = 0;
  for (let i = 0; i < N; i++) {
    if (i === N / 2) {
      Object.defineProperty(P, "flag", {
        get: function (this: any) {
          return this.k * 50;
        },
        configurable: true,
      });
    }
    sum += readFlag(o);
  }
  console.log("spill_lane_getter", sum);
}
