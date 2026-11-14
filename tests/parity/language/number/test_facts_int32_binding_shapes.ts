// A value the whole-program facts prove an int32 is held as one whatever
// binding carries it, and check mode (`PERRY_VERIFY_FACTS=1`) tests the
// claim where the value enters the int32 slot. Each shape below computes
// values on both sides of the int32 boundaries (2^31 - 1, 2^31, -2^31,
// -2^31 - 1), `-0`, `NaN` and a fraction from inputs the compiler cannot
// fold. Every line prints what node prints; none is a false proof.
const RUN = Math.random() < 2 ? 1 : 0;
const many = process.argv.length > 100;

function show(v: number): string {
  return Object.is(v, -0) ? "-0" : String(v);
}
// The inputs: small ints, and the offsets that carry them over each edge.
const BASES: number[] = [];
for (let i = 0; i < 4 * RUN; i++) BASES.push(i);
const EDGES: number[] = [0, 7, 2147483644, 2147483647, 2147483648, -2147483648, -2147483651, 3000000000];
const out: string[] = [];
function emit(tag: string, v: number): void {
  out.push(tag + "=" + show(v));
}

// (1) a `const` read once, as a value and under an int use.
function constOnce(i: number, e: number): number {
  const k = i + e;
  return k;
}
function constOnceInt(i: number, e: number): number {
  const k = i + e;
  return k === 7 ? -1 : k * 0.5;
}
// (2) a `let` read once.
function letOnce(i: number, e: number): number {
  let k = i + e;
  return k * 0.5;
}
// (3) destructured bindings.
function destructured(i: number, e: number): number {
  const [p, q] = [i + e, i - e];
  const { r } = { r: p * 2 };
  return p === 7 ? q : r - q;
}
// (4) a parameter default.
function withDefault(i: number, e: number, k: number = i + e): number {
  return k === 7 ? -1 : k + 0.25;
}
// (5) compound assignment targets and updates.
function compound(i: number, e: number): number {
  let k = i;
  k += e;
  k -= 1;
  k *= 1;
  let n = e;
  n++;
  n++;
  --n;
  return (k === 7 ? 1 : 0) + k * 2 + n;
}
// (6) field initializers and field stores.
class Cell {
  k: number;
  m: number = 0;
  constructor(i: number, e: number) {
    this.k = i + e;
  }
  bump(d: number): number {
    this.m = this.k + d;
    return this.m === 7 ? -1 : this.m;
  }
}
// (7) ternary and logical arms.
function arms(i: number, e: number): number {
  const z = i > 1 ? 0 : i;
  const k = i > 100 ? 1 : i + e;
  const l = z || i + e;
  const n = (many ? undefined : i + e) ?? 5;
  return (k === 7 ? 1 : 0) + k + l * 0.5 + n * 0.25;
}
// (8) call arguments and return values.
function taken(k: number): number {
  return k === 7 ? -1 : k * 0.5;
}
function made(i: number, e: number): number {
  return i + e;
}
function passes(i: number, e: number): number {
  const got = made(i, e);
  return taken(i + e) + (got === 7 ? 1 : got);
}
// (9) a `const` a closure captures, and a closure's own `const`.
function captured(i: number, e: number): number {
  const k = i + e;
  const read = (): number => (k === 7 ? -1 : k * 0.5);
  const own = (j: number): number => {
    const d = j + e;
    return d === 7 ? -1 : d * 0.25;
  };
  return read() + own(i);
}
// (10) static methods, methods, a generator body.
class Calc {
  static twice(i: number, e: number): number {
    const d = i + e;
    return d === 7 ? -1 : d * 2;
  }
  half(i: number, e: number): number {
    const d = i + e;
    return d === 7 ? -1 : d * 0.5;
  }
}
function* steps(n: number, e: number): Generator<number> {
  for (let i = 0; i < n; i++) {
    const d = i + e;
    yield d === 7 ? -1 : d * 0.5;
  }
}
// (11) loop-local `const`s, an accumulator, a counter that crosses 2^31.
function looped(n: number, e: number): number {
  let acc = 0;
  for (let i = 0; i < n; i++) {
    const d = i + e;
    acc = acc * 2 - d;
    if (d === 7) acc += 1;
  }
  let c = e;
  for (let i = 0; i < n; i++) c++;
  return acc + c * 0.5;
}
// (12) `-0`, `NaN` and fractions through the same shapes.
function odd(i: number): string {
  const z = i * -0;
  const n = (i - i) / (i - i);
  const f = i + 0.5;
  const k = many ? 1 : z;
  let u = z;
  u += z;
  const w = Math.round(-0.25 * (i + 1));
  return show(z) + show(n) + show(f) + show(k) + show(u) + show(w) + show(taken(z)) + show(made(z, z));
}

const calc = new Calc();
for (const e of EDGES) {
  for (const i of BASES) {
    emit("c1", constOnce(i, e));
    emit("c2", constOnceInt(i, e));
    emit("l1", letOnce(i, e));
    emit("d1", destructured(i, e));
    emit("p1", withDefault(i, e));
    emit("p2", withDefault(i, e, e - i));
    emit("a1", compound(i, e));
    emit("f1", new Cell(i, e).bump(i));
    emit("t1", arms(i, e));
    emit("r1", passes(i, e));
    emit("k1", captured(i, e));
    emit("s1", Calc.twice(i, e));
    emit("m1", calc.half(i, e));
    emit("o1", looped(i, e));
  }
  for (const v of steps(3 * RUN, e)) emit("g1", v);
  // (13) module-level bindings: a `const` and a `let` of the module body,
  // and a `const` of a loop in it.
  const top = e + RUN;
  let acc = top;
  acc += e;
  const once = top * 1 + 2;
  emit("u1", top === 7 ? -1 : top);
  emit("u2", acc);
  emit("u3", once);
}
for (const i of BASES) out.push("z" + i + "=" + odd(i));
console.log(out.length);
for (let i = 0; i < out.length; i += 8) console.log(out.slice(i, i + 8).join(" "));
