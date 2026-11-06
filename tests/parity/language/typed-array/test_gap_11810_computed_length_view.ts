// #11810: a typed array whose length is an operator result (never an Object)
// is the length form: fresh inline storage, exactly like a literal length.
// The n-body kernel over `new Float64Array(rows.length * 7)`, plus the edge
// values an operator can produce as a length.
const PI = Math.PI;
const SOLAR_MASS = 4 * PI * PI;
const DAYS_PER_YEAR = 365.24;
const rows: number[][] = [
  [0, 0, 0, 0, 0, 0, SOLAR_MASS],
  [4.84143144246472090e+00, -1.16032004402742839e+00, -1.03622044471123109e-01, 1.66007664274403694e-03 * DAYS_PER_YEAR, 7.69901118419740425e-03 * DAYS_PER_YEAR, -6.90460016972063023e-05 * DAYS_PER_YEAR, 9.54791938424326609e-04 * SOLAR_MASS],
  [8.34336671824457987e+00, 4.12479856412430479e+00, -4.03523417114321381e-01, -2.76742510726862411e-03 * DAYS_PER_YEAR, 4.99852801234917238e-03 * DAYS_PER_YEAR, 2.30417297573763929e-05 * DAYS_PER_YEAR, 2.85885980666130812e-04 * SOLAR_MASS],
  [1.28943695621391310e+01, -1.51111514016986312e+01, -2.23307578892655734e-01, 2.96460137564761618e-03 * DAYS_PER_YEAR, 2.37847173959480950e-03 * DAYS_PER_YEAR, -2.96589568540237556e-05 * DAYS_PER_YEAR, 4.36624404335156298e-05 * SOLAR_MASS],
  [1.53796971148509165e+01, -2.59193146099879641e+01, 1.79258772950371181e-01, 2.68067772490389322e-03 * DAYS_PER_YEAR, 1.62824170038242295e-03 * DAYS_PER_YEAR, -9.51592254519715870e-05 * DAYS_PER_YEAR, 5.15138902046611451e-05 * SOLAR_MASS],
];

function advance(a: Float64Array, dt: number): void {
  for (let i = 0; i < 5; i++) {
    const oi = i * 7;
    for (let j = i + 1; j < 5; j++) {
      const oj = j * 7;
      const dx = a[oi] - a[oj], dy = a[oi + 1] - a[oj + 1], dz = a[oi + 2] - a[oj + 2];
      const d2 = dx * dx + dy * dy + dz * dz;
      const mag = dt / (d2 * Math.sqrt(d2));
      const bim = a[oi + 6] * mag, bjm = a[oj + 6] * mag;
      a[oi + 3] -= dx * bjm; a[oi + 4] -= dy * bjm; a[oi + 5] -= dz * bjm;
      a[oj + 3] += dx * bim; a[oj + 4] += dy * bim; a[oj + 5] += dz * bim;
    }
  }
  for (let i = 0; i < 5; i++) {
    const o = i * 7;
    a[o] += dt * a[o + 3]; a[o + 1] += dt * a[o + 4]; a[o + 2] += dt * a[o + 5];
  }
}

function energy(a: Float64Array): number {
  let e = 0;
  for (let i = 0; i < 5; i++) {
    const oi = i * 7;
    e += 0.5 * a[oi + 6] * (a[oi + 3] * a[oi + 3] + a[oi + 4] * a[oi + 4] + a[oi + 5] * a[oi + 5]);
    for (let j = i + 1; j < 5; j++) {
      const oj = j * 7;
      const dx = a[oi] - a[oj], dy = a[oi + 1] - a[oj + 1], dz = a[oi + 2] - a[oj + 2];
      e -= (a[oi + 6] * a[oj + 6]) / Math.sqrt(dx * dx + dy * dy + dz * dz);
    }
  }
  return e;
}

const a = new Float64Array(rows.length * 7);
for (let i = 0; i < 5; i++) for (let k = 0; k < 7; k++) a[i * 7 + k] = rows[i][k];
console.log("len", a.length, energy(a).toFixed(9));
for (let i = 0; i < 1000; i++) advance(a, 0.01);
console.log("after", energy(a).toFixed(9));
a[35] = 1;
console.log("oob", a[35], a.length);

// Through an immutable local, and a value the operator produces from a string.
const n = rows.length * 2;
const b = new Int32Array(n);
for (let i = 0; i < b.length; i++) b[i] = i * 3;
console.log("b", b.length, b[9], b[10]);
const s: any = "4";
const c = new Uint8Array(s * 1);
c[3] = 300;
console.log("c", c.length, c[3], c[4]);

// Other operators: a comparison is a boolean (length 1), `typeof` a string
// (NaN, length 0), a unary minus of a negative length.
const d = new Float64Array(+(rows.length > 2));
d[0] = 2.5;
console.log("d", d.length, d[0]);
const e = new Float32Array(typeof s as any);
console.log("e", e.length);
const neg = -3;
const f = new Uint16Array(-neg);
f[2] = 70000;
console.log("f", f.length, f[2]);

// The forms an Object argument selects stay the view and copy forms: the
// view shares the buffer's bytes, the copy takes the source's elements.
const buf = new ArrayBuffer(16);
const bufAny: any = buf;
const view = new Float64Array(bufAny);
const alias = new Float64Array(buf);
view[1] = 9.5;
console.log("view", view.length, alias[1]);
const copy = new Float64Array(rows[1]);
copy[0] = -1;
console.log("copy", copy.length, copy[0], rows[1][0].toFixed(3));

// Lengths an operator yields that the constructor must reject.
try {
  const g = new Float64Array(neg * 1);
  console.log("g", g.length);
} catch (err) {
  console.log("g throws", (err as Error).name);
}
try {
  const big: any = 2n;
  const h = new Float64Array(big * 3n);
  console.log("h", h.length);
} catch (err) {
  console.log("h throws", (err as Error).name);
}
