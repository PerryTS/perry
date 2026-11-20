// #11810: a second name for a proven typed-array view keeps the view.
//
// `a[k] -= v` spills its receiver into a compiler temp (`let base = a`), and
// a `const t = a` written by hand is the same alias. Both names denote one
// storage, so they share the view (same cached data pointer, same alias
// scope). This file checks the behaviour that sharing must keep: every
// write through one name is visible through the other, a reassigned alias no
// longer reads the old storage, `.buffer` exposure through either name
// reaches both, and the n-body kernel from the issue (a module-level
// `new Float64Array(35)` driving a specialized clone) prints the same energy
// as node.

const PI = Math.PI;
const SOLAR_MASS = 4 * PI * PI;
const DAYS_PER_YEAR = 365.24;
const D: number[][] = [
  [0, 0, 0, 0, 0, 0, SOLAR_MASS],
  [4.84143144246472090e+00, -1.16032004402742839e+00, -1.03622044471123109e-01, 1.66007664274403694e-03 * DAYS_PER_YEAR, 7.69901118419740425e-03 * DAYS_PER_YEAR, -6.90460016972063023e-05 * DAYS_PER_YEAR, 9.54791938424326609e-04 * SOLAR_MASS],
  [8.34336671824457987e+00, 4.12479856412430479e+00, -4.03523417114321381e-01, -2.76742510726862411e-03 * DAYS_PER_YEAR, 4.99852801234917238e-03 * DAYS_PER_YEAR, 2.30417297573763929e-05 * DAYS_PER_YEAR, 2.85885980666130812e-04 * SOLAR_MASS],
  [1.28943695621391310e+01, -1.51111514016986312e+01, -2.23307578892655734e-01, 2.96460137564761618e-03 * DAYS_PER_YEAR, 2.37847173959480950e-03 * DAYS_PER_YEAR, -2.96589568540237556e-05 * DAYS_PER_YEAR, 4.36624404335156298e-05 * SOLAR_MASS],
  [1.53796971148509165e+01, -2.59193146099879641e+01, 1.79258772950371181e-01, 2.68067772490389322e-03 * DAYS_PER_YEAR, 1.62824170038242295e-03 * DAYS_PER_YEAR, -9.51592254519715870e-05 * DAYS_PER_YEAR, 5.15138902046611451e-05 * SOLAR_MASS],
];

function offsetMomentum(a: Float64Array): void {
  let px = 0, py = 0, pz = 0;
  for (let i = 0; i < 5; i++) {
    const o = i * 7;
    px += a[o + 3] * a[o + 6]; py += a[o + 4] * a[o + 6]; pz += a[o + 5] * a[o + 6];
  }
  a[3] = -px / SOLAR_MASS; a[4] = -py / SOLAR_MASS; a[5] = -pz / SOLAR_MASS;
}

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

// The issue's shape: literal length, module scope, element access through a
// parameter in a specialized clone.
const bodies = new Float64Array(35);
for (let i = 0; i < 5; i++) for (let k = 0; k < 7; k++) bodies[i * 7 + k] = D[i][k];
offsetMomentum(bodies);
console.log("nbody start", energy(bodies).toFixed(9));
for (let i = 0; i < 1000; i++) advance(bodies, 0.01);
console.log("nbody end", energy(bodies).toFixed(9));

// Writes through a hand-written alias are visible through the source, and the
// other way round; compound updates go through both names.
function viaAlias(a: Float64Array): string {
  const t = a;
  t[1] = 10;
  a[2] = 20;
  t[1] += a[2];
  a[2] -= t[1];
  for (let i = 0; i < 4; i++) { t[i] += 1; a[i] *= 2; }
  return Array.from(a).join(",") + " | " + Array.from(t).join(",");
}
console.log("alias", viaAlias(new Float64Array(4)));
const lit4 = new Float64Array(4);
console.log("alias literal", viaAlias(lit4), lit4[1]);

// A mutable alias that is reassigned must stop reading the old storage, and
// the source must keep reading its own.
function reassignedAlias(): string {
  const a = new Float64Array(4);
  let t = a;
  t[0] = 1;
  t = new Float64Array(4);
  t[0] = 2;
  a[1] = 3;
  t[1] = 4;
  return Array.from(a).join(",") + " | " + Array.from(t).join(",");
}
console.log("reassigned alias", reassignedAlias());

function reassignedSource(): string {
  let a = new Float64Array(3);
  const t = a;
  a[0] = 5;
  a = new Float64Array(3);
  a[0] = 6;
  t[1] = 7;
  return Array.from(a).join(",") + " | " + Array.from(t).join(",");
}
console.log("reassigned source", reassignedSource());

function reassignedU8(): string {
  const a = new Uint8Array(4);
  let t = a;
  t[0] = 1;
  t = new Uint8Array(4);
  t[0] = 9;
  a[2] = 3;
  return Array.from(a).join(",") + " | " + Array.from(t).join(",");
}
console.log("reassigned u8", reassignedU8());

// `.buffer` through the alias exposes the storage of BOTH names.
function bufferThroughAlias(): string {
  const a = new Float64Array(4);
  const t = a;
  a[0] = 1;
  const u = new Float64Array(t.buffer);
  u[1] = 5;
  a[2] = 6;
  u[3] = u[2] + a[0];
  return Array.from(a).join(",") + " | " + Array.from(t).join(",");
}
console.log("buffer via alias", bufferThroughAlias());

function bufferThroughSource(): string {
  const a = new Int32Array(4);
  const t = a;
  const bytes = new Uint8Array(a.buffer);
  t[0] = 258;
  bytes[4] = 7;
  return Array.from(t).join(",") + " | " + bytes[0] + "," + bytes[1];
}
console.log("buffer via source", bufferThroughSource());

// An alias captured by a closure, written there, read through the source.
function capturedAlias(): string {
  const a = new Float64Array(3);
  const t = a;
  const bump = (k: number) => { t[k] += 10; };
  a[0] = 1;
  bump(0);
  bump(2);
  a[2] -= 1;
  return Array.from(a).join(",");
}
console.log("captured alias", capturedAlias());

// Out-of-bounds and non-integer keys through the compound-assignment alias.
function oddKeys(a: Float64Array): string {
  const k = 4;
  a[k] -= 1;
  a[1.5] -= 1;
  a[-1] += 1;
  a[0] -= 1;
  return Array.from(a).join(",") + " " + a[k] + " " + a[1.5];
}
console.log("odd keys", oddKeys(new Float64Array(4)));


// Unproven keys through a hand-written alias in an unspecialized body: the
// alias takes the bounds-guarded native tier and must agree with node for
// out-of-range, fractional, negative, NaN, -0 and string keys.
function aliasKeys(k: any): string {
  const src = new Uint16Array(4);
  const alias = src;
  src[0] = 258;
  alias[k] = 513;
  return src[0] + "," + alias[k] + "," + src[k] + "," + Array.from(src).join("/");
}
for (const k of [0, 1, 3, 4, -1, 1.5, NaN, 1e10, -0, "2", "x", undefined] as any[]) {
  console.log("alias key", String(k), aliasKeys(k));
}
