// #11086: a class nested in a function whose STATIC field initializer runs
// `new Self(...)` while the class is being defined, and whose methods capture a
// `const`/`let` declared AFTER the class. The `new` forwards the class's
// captures as trailing ctor args; those forwards must not throw the TDZ
// ReferenceError (the methods only read the binding later, after it is
// initialized). Shape of @noble/curves' `weierstrassPoints` + `const wnaf`,
// which `new ethers.Wallet(pk)` hits.

function wNAF(bits: number) {
  return {
    mul(n: number): string {
      return "wnaf(" + bits + ")*" + n;
    },
  };
}

function makeCurve(g: number, bits: number) {
  class Point {
    static readonly BASE = new Point(g);
    static readonly ZERO = new Point(0);
    x: number;
    constructor(x: number) {
      this.x = x;
    }
    multiply(n: number): string {
      return wnaf.mul(n) + "@" + this.x + " tag=" + tag;
    }
    static describe(): string {
      return "curve bits=" + bits + " wnaf=" + typeof wnaf;
    }
  }
  const wnaf = wNAF(bits);
  let tag = "t" + bits;
  return Point;
}

// One factory call only: a function-nested class declaration evaluated twice
// is a separate, pre-existing divergence (both calls share one class).
const A = makeCurve(5, 256);
console.log(A.BASE.multiply(3));
console.log(A.ZERO.multiply(1));
console.log(new A(9).multiply(4));
console.log(A.describe());
console.log(A.BASE instanceof A, A.BASE === A.ZERO);
