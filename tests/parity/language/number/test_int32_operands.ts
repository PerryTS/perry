// A bitwise operator whose operands the facts hold as int32 words (an I32
// field lane, an int32 element read) stays in i32 (PERRY_INT32_OPERANDS).
// Exact against node with the rule on, off and in check mode.
// A loop bound the compiler cannot fold keeps each loop as written.
const RUN = Math.random() < 2 ? 1 : 0;

// 1. The shape: two hash lanes updated from two int32 tables, with keys
// that can miss (`?? 0`).
class Hasher {
  za: number[] = [];
  zb: number[] = [];
  ha = 0;
  hb = 0;
  n = 0;
  constructor() {
    let seed = 0x6d2b79f5;
    for (let i = 0; i < 512; i++) {
      seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5;
      this.za.push(seed);
      seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5;
      this.zb.push(seed);
    }
  }
  mix(k: number): void {
    this.ha ^= this.za[k] ?? 0;
    this.hb ^= this.zb[k] ?? 0;
    this.n = (this.n + 1) | 0;
  }
  fold(): number {
    return (this.ha ^ this.hb) | 0;
  }
  ops(k: number): number {
    const a = this.ha & (this.za[k] ?? 0);
    const b = this.hb | (this.zb[k] ?? 0);
    const c = this.ha << (this.n & 7);
    const d = this.hb >> (this.n & 7);
    return (a ^ b ^ c ^ d) | 0;
  }
}
const h = new Hasher();
let acc1 = 0;
for (let i = 0; i < 2000 * RUN; i++) {
  h.mix((i * 7) % 600 - 20);
  acc1 = (acc1 + h.ops(i % 512)) | 0;
}
console.log("hasher", h.ha, h.hb, h.n, h.fold(), acc1);

// 2. A table with a fraction, -0, NaN and a value past int32: no int32
// read, the generic operator runs (ToInt32 of each).
class Loose {
  t: number[] = [1.5, -0, NaN, 4294967301, -7, 2147483648, 1e20];
  h = 0;
  mix(k: number): void {
    this.h ^= this.t[k] ?? 0;
  }
}
const l = new Loose();
const steps: number[] = [];
for (let i = 0; i < 9 * RUN; i++) {
  l.mix(i - 1);
  steps.push(l.h);
}
console.log("loose", steps.join(","));

// 3. A lane that leaves int32 (a double field) beside an int32 table.
class Wide {
  t: number[] = [];
  h = 0.5;
  constructor() {
    for (let i = 0; i < 16; i++) this.t.push(i * 1000003);
  }
  mix(k: number): number {
    this.h = this.h * 3 + 0.25;
    return (this.h ^ (this.t[k] ?? 0)) | 0;
  }
}
const wd = new Wide();
let acc3 = 0;
for (let i = 0; i < 40 * RUN; i++) acc3 = (acc3 + wd.mix(i % 20)) | 0;
console.log("wide", acc3, wd.h);

// 4. Other defaults: a non-literal default, a string default, no default
// (`undefined` operand is ToInt32 0), a BigInt table.
class Defaults {
  t: number[] = [];
  h = 0;
  constructor() {
    for (let i = 0; i < 8; i++) this.t.push((i * 2654435761) | 0);
  }
  run(k: number, d: number): number {
    this.h ^= this.t[k] ?? d;
    this.h ^= this.t[k] ?? ("7" as any);
    this.h ^= this.t[k] as number;
    return this.h;
  }
}
const df = new Defaults();
const out4: number[] = [];
for (let i = 0; i < 12 * RUN; i++) out4.push(df.run(i - 2, i * 3));
console.log("defaults", out4.join(","));
const big: bigint[] = [1n, 2n, 3n];
let bh = 0n;
for (let i = 0; i < 3 * RUN; i++) bh ^= big[i] ?? 0n;
let threw = "";
try {
  let m: any = 5;
  for (let i = 0; i < 1 * RUN; i++) m ^= big[i] as any;
  threw = String(m);
} catch (e) {
  threw = (e as Error).name;
}
console.log("bigint", String(bh), threw);

// 5. A named property and a hole on the table: a key below 0 reads the
// property, a hole reads `undefined`.
class Holes {
  t: number[] = [];
  h = 0;
  constructor() {
    for (let i = 0; i < 8; i++) this.t.push(i + 100);
    (this.t as any)[-1] = 77.5;
    this.t[12] = 5;
  }
  mix(k: number): void {
    this.h ^= this.t[k] ?? 0;
  }
}
const ho = new Holes();
const out5: number[] = [];
for (let i = 0; i < 16 * RUN; i++) {
  ho.mix(i - 2);
  out5.push(ho.h);
}
console.log("holes", out5.join(","));
