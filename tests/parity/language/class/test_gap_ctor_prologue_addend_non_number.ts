// A constructor that is a run of `this.f = p` / `this.f = p + <literal>`
// stores may skip its call and store the fields at the `new` site. The
// `p + 1` there is JS `+`: a `number` annotation does not make the argument
// a Number, so a string must still concatenate and an object must still
// convert through `valueOf`/`toString`.

class Wide {
  x: number;
  a: number;
  b: number;
  c: number;
  d: number;
  constructor(x: number, a: number, b: number, c: number, d: number) {
    this.x = x + 1;
    this.a = a;
    this.b = b;
    this.c = c;
    this.d = d;
  }
}

class Narrow {
  x: number;
  a: number;
  constructor(x: number, a: number) {
    this.x = x + 1;
    this.a = a;
  }
}

class Offsets {
  lo: number;
  hi: number;
  constructor(base: number) {
    this.lo = base + 0.5;
    this.hi = base + 100;
  }
}

const s: any = "s";
const o: any = { valueOf: () => 41 };
const t: any = { toString: () => "t" };
const u: any = undefined;
const big: any = 1e308;

console.log(new Wide(s, 1, 2, 3, 4).x);
console.log(new Narrow(s, 1).x);
console.log(new Offsets(s).lo, new Offsets(s).hi);
console.log(new Wide(o, 1, 2, 3, 4).x, new Narrow(o, 0).x);
console.log(new Wide(t, 1, 2, 3, 4).x, new Narrow(t, 0).x);
console.log(new Wide(u, 1, 2, 3, 4).x, new Narrow(u, 0).x);
console.log(new Wide(big * 10, 1, 2, 3, 4).x, new Narrow(-big * 10, 0).x);
console.log(new Wide(NaN, 1, 2, 3, 4).x, new Narrow(true as any, 0).x);
console.log(typeof new Wide(s, 1, 2, 3, 4).x, typeof new Narrow(s, 1).x);

// Numbers keep the stored-at-birth result.
let sum = 0;
for (let i = 0; i < 1000; i++) {
  const w = new Wide(i, i, i, i, i);
  const n = new Narrow(i * 0.5, i);
  const f = new Offsets(i);
  sum += w.x + w.d + n.x + n.a + f.lo + f.hi;
}
console.log(sum);

// A value that is sometimes a string, at one site.
const mixed: any[] = [1, "2", 3.5, "x", 5];
const out: string[] = [];
for (const v of mixed) {
  out.push(String(new Wide(v, 0, 0, 0, 0).x), String(new Narrow(v, 0).x));
}
console.log(out.join(","));
