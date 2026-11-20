// Compound updates on typed-array elements: `x[i]++`, `x[i]--`, `++x[i]`,
// `--x[i]` and every `op=` form, on every element type. The update must apply
// ToNumeric to the element, step it as a Number (or BigInt), and store it with
// the element type's own conversion: integer kinds wrap, Uint8Clamped clamps
// with round-half-to-even, float kinds round to their width. The expression's
// value is the unconverted Number. Out-of-bounds and non-integer indices read
// `undefined` and write nothing, and never consult the prototype.

function show(label: string, v: unknown): void {
  if (typeof v === "number" && Object.is(v, -0)) {
    console.log(label, "-0");
  } else if (typeof v === "bigint") {
    console.log(label, v.toString() + "n");
  } else {
    console.log(label, v);
  }
}

function dump(label: string, ta: ArrayLike<unknown>): void {
  const parts: string[] = [];
  for (let i = 0; i < ta.length; i++) {
    const v = ta[i];
    parts.push(typeof v === "bigint" ? v.toString() + "n" : Object.is(v, -0) ? "-0" : String(v));
  }
  console.log(label, parts.join(","));
}

// ---- increments and decrements on every numeric kind ----
function stepAll(name: string, a: Int8Array | Uint8Array | Uint8ClampedArray | Int16Array | Uint16Array | Int32Array | Uint32Array | Float32Array | Float64Array, lo: number, hi: number): void {
  a[0] = hi;
  show(name + " post++ at max", a[0]++);
  show(name + " after", a[0]);
  a[1] = lo;
  show(name + " post-- at min", a[1]--);
  show(name + " after", a[1]);
  a[2] = hi;
  show(name + " pre++ at max", ++a[2]);
  show(name + " after", a[2]);
  a[3] = lo;
  show(name + " pre-- at min", --a[3]);
  show(name + " after", a[3]);
  a[4] = 7;
  a[4] += 2.75;
  show(name + " += 2.75", a[4]);
  a[4] -= 20;
  show(name + " -= 20", a[4]);
  a[4] *= 3;
  show(name + " *= 3", a[4]);
  a[4] /= 2;
  show(name + " /= 2", a[4]);
  a[4] %= 5;
  show(name + " %= 5", a[4]);
  a[5] = 3;
  a[5] **= 4;
  show(name + " **= 4", a[5]);
  a[5] <<= 3;
  show(name + " <<= 3", a[5]);
  a[5] >>= 1;
  show(name + " >>= 1", a[5]);
  a[5] >>>= 2;
  show(name + " >>>= 2", a[5]);
  a[5] |= 0x13;
  show(name + " |= 0x13", a[5]);
  a[5] &= 0x1e;
  show(name + " &= 0x1e", a[5]);
  a[5] ^= 0xff;
  show(name + " ^= 0xff", a[5]);
  a[6] = 1.5;
  a[6]++;
  show(name + " 1.5++", a[6]);
  a[7] = NaN;
  a[7]++;
  show(name + " NaN++", a[7]);
  show(name + " post-- of NaN elem", a[7]--);
  // out of bounds: the value is NaN, nothing is written, length is unchanged
  show(name + " oob post++", a[100]++);
  show(name + " oob pre--", --a[100]);
  show(name + " neg post--", a[-1]--);
  show(name + " frac pre++", ++a[1.5]);
  a[1.5] += 3;
  show(name + " frac read", a[1.5]);
  show(name + " oob read", a[100]);
  show(name + " length", a.length);
  dump(name + " final", a);
}

stepAll("Int8", new Int8Array(8), -128, 127);
stepAll("Uint8", new Uint8Array(8), 0, 255);
stepAll("Uint8Clamped", new Uint8ClampedArray(8), 0, 255);
stepAll("Int16", new Int16Array(8), -32768, 32767);
stepAll("Uint16", new Uint16Array(8), 0, 65535);
stepAll("Int32", new Int32Array(8), -2147483648, 2147483647);
stepAll("Uint32", new Uint32Array(8), 0, 4294967295);
stepAll("Float32", new Float32Array(8), -3.4028234663852886e38, 16777216);
stepAll("Float64", new Float64Array(8), -9007199254740992, 9007199254740992);
stepAll("Float16", new (globalThis as any).Float16Array(8), -65504, 2048);

// ---- the same checks on a local receiver of each kind (the inline path) ----
function localInt8(): void {
  const a = new Int8Array(8);
  const n = "local Int8";
  a[0] = 127;
  show(n + " post++ at max", a[0]++);
  a[1] = -128;
  show(n + " post-- at min", a[1]--);
  a[2] = 127;
  show(n + " pre++ at max", ++a[2]);
  a[3] = -128;
  show(n + " pre-- at min", --a[3]);
  a[4] = 1.5;
  a[4]++;
  a[5] = NaN;
  show(n + " NaN post++", a[5]++);
  show(n + " NaN pre--", --a[5]);
  let k = 100;
  show(n + " oob post++", a[k]++);
  show(n + " oob pre--", --a[k]);
  k = -1;
  show(n + " neg post--", a[k]--);
  let f = 1.5;
  show(n + " frac pre++", ++a[f]);
  show(n + " frac read", a[f]);
  for (let i = 0; i < 8; i++) a[i]--;
  for (let i = 7; i >= 0; i--) ++a[i];
  for (let i = 0; i < 8; i++) a[i]--;
  show(n + " length", a.length);
  dump(n + " final", a);
}
localInt8();
function localUint8(): void {
  const a = new Uint8Array(8);
  const n = "local Uint8";
  a[0] = 255;
  show(n + " post++ at max", a[0]++);
  a[1] = 0;
  show(n + " post-- at min", a[1]--);
  a[2] = 255;
  show(n + " pre++ at max", ++a[2]);
  a[3] = 0;
  show(n + " pre-- at min", --a[3]);
  a[4] = 1.5;
  a[4]++;
  a[5] = NaN;
  show(n + " NaN post++", a[5]++);
  show(n + " NaN pre--", --a[5]);
  let k = 100;
  show(n + " oob post++", a[k]++);
  show(n + " oob pre--", --a[k]);
  k = -1;
  show(n + " neg post--", a[k]--);
  let f = 1.5;
  show(n + " frac pre++", ++a[f]);
  show(n + " frac read", a[f]);
  for (let i = 0; i < 8; i++) a[i]--;
  for (let i = 7; i >= 0; i--) ++a[i];
  for (let i = 0; i < 8; i++) a[i]--;
  show(n + " length", a.length);
  dump(n + " final", a);
}
localUint8();
function localUint8Clamped(): void {
  const a = new Uint8ClampedArray(8);
  const n = "local Uint8Clamped";
  a[0] = 255;
  show(n + " post++ at max", a[0]++);
  a[1] = 0;
  show(n + " post-- at min", a[1]--);
  a[2] = 255;
  show(n + " pre++ at max", ++a[2]);
  a[3] = 0;
  show(n + " pre-- at min", --a[3]);
  a[4] = 1.5;
  a[4]++;
  a[5] = NaN;
  show(n + " NaN post++", a[5]++);
  show(n + " NaN pre--", --a[5]);
  let k = 100;
  show(n + " oob post++", a[k]++);
  show(n + " oob pre--", --a[k]);
  k = -1;
  show(n + " neg post--", a[k]--);
  let f = 1.5;
  show(n + " frac pre++", ++a[f]);
  show(n + " frac read", a[f]);
  for (let i = 0; i < 8; i++) a[i]--;
  for (let i = 7; i >= 0; i--) ++a[i];
  for (let i = 0; i < 8; i++) a[i]--;
  show(n + " length", a.length);
  dump(n + " final", a);
}
localUint8Clamped();
function localInt16(): void {
  const a = new Int16Array(8);
  const n = "local Int16";
  a[0] = 32767;
  show(n + " post++ at max", a[0]++);
  a[1] = -32768;
  show(n + " post-- at min", a[1]--);
  a[2] = 32767;
  show(n + " pre++ at max", ++a[2]);
  a[3] = -32768;
  show(n + " pre-- at min", --a[3]);
  a[4] = 1.5;
  a[4]++;
  a[5] = NaN;
  show(n + " NaN post++", a[5]++);
  show(n + " NaN pre--", --a[5]);
  let k = 100;
  show(n + " oob post++", a[k]++);
  show(n + " oob pre--", --a[k]);
  k = -1;
  show(n + " neg post--", a[k]--);
  let f = 1.5;
  show(n + " frac pre++", ++a[f]);
  show(n + " frac read", a[f]);
  for (let i = 0; i < 8; i++) a[i]--;
  for (let i = 7; i >= 0; i--) ++a[i];
  for (let i = 0; i < 8; i++) a[i]--;
  show(n + " length", a.length);
  dump(n + " final", a);
}
localInt16();
function localUint16(): void {
  const a = new Uint16Array(8);
  const n = "local Uint16";
  a[0] = 65535;
  show(n + " post++ at max", a[0]++);
  a[1] = 0;
  show(n + " post-- at min", a[1]--);
  a[2] = 65535;
  show(n + " pre++ at max", ++a[2]);
  a[3] = 0;
  show(n + " pre-- at min", --a[3]);
  a[4] = 1.5;
  a[4]++;
  a[5] = NaN;
  show(n + " NaN post++", a[5]++);
  show(n + " NaN pre--", --a[5]);
  let k = 100;
  show(n + " oob post++", a[k]++);
  show(n + " oob pre--", --a[k]);
  k = -1;
  show(n + " neg post--", a[k]--);
  let f = 1.5;
  show(n + " frac pre++", ++a[f]);
  show(n + " frac read", a[f]);
  for (let i = 0; i < 8; i++) a[i]--;
  for (let i = 7; i >= 0; i--) ++a[i];
  for (let i = 0; i < 8; i++) a[i]--;
  show(n + " length", a.length);
  dump(n + " final", a);
}
localUint16();
function localInt32(): void {
  const a = new Int32Array(8);
  const n = "local Int32";
  a[0] = 2147483647;
  show(n + " post++ at max", a[0]++);
  a[1] = -2147483648;
  show(n + " post-- at min", a[1]--);
  a[2] = 2147483647;
  show(n + " pre++ at max", ++a[2]);
  a[3] = -2147483648;
  show(n + " pre-- at min", --a[3]);
  a[4] = 1.5;
  a[4]++;
  a[5] = NaN;
  show(n + " NaN post++", a[5]++);
  show(n + " NaN pre--", --a[5]);
  let k = 100;
  show(n + " oob post++", a[k]++);
  show(n + " oob pre--", --a[k]);
  k = -1;
  show(n + " neg post--", a[k]--);
  let f = 1.5;
  show(n + " frac pre++", ++a[f]);
  show(n + " frac read", a[f]);
  for (let i = 0; i < 8; i++) a[i]--;
  for (let i = 7; i >= 0; i--) ++a[i];
  for (let i = 0; i < 8; i++) a[i]--;
  show(n + " length", a.length);
  dump(n + " final", a);
}
localInt32();
function localUint32(): void {
  const a = new Uint32Array(8);
  const n = "local Uint32";
  a[0] = 4294967295;
  show(n + " post++ at max", a[0]++);
  a[1] = 0;
  show(n + " post-- at min", a[1]--);
  a[2] = 4294967295;
  show(n + " pre++ at max", ++a[2]);
  a[3] = 0;
  show(n + " pre-- at min", --a[3]);
  a[4] = 1.5;
  a[4]++;
  a[5] = NaN;
  show(n + " NaN post++", a[5]++);
  show(n + " NaN pre--", --a[5]);
  let k = 100;
  show(n + " oob post++", a[k]++);
  show(n + " oob pre--", --a[k]);
  k = -1;
  show(n + " neg post--", a[k]--);
  let f = 1.5;
  show(n + " frac pre++", ++a[f]);
  show(n + " frac read", a[f]);
  for (let i = 0; i < 8; i++) a[i]--;
  for (let i = 7; i >= 0; i--) ++a[i];
  for (let i = 0; i < 8; i++) a[i]--;
  show(n + " length", a.length);
  dump(n + " final", a);
}
localUint32();
function localFloat32(): void {
  const a = new Float32Array(8);
  const n = "local Float32";
  a[0] = 16777216;
  show(n + " post++ at max", a[0]++);
  a[1] = -3.4028234663852886e38;
  show(n + " post-- at min", a[1]--);
  a[2] = 16777216;
  show(n + " pre++ at max", ++a[2]);
  a[3] = -3.4028234663852886e38;
  show(n + " pre-- at min", --a[3]);
  a[4] = 1.5;
  a[4]++;
  a[5] = NaN;
  show(n + " NaN post++", a[5]++);
  show(n + " NaN pre--", --a[5]);
  let k = 100;
  show(n + " oob post++", a[k]++);
  show(n + " oob pre--", --a[k]);
  k = -1;
  show(n + " neg post--", a[k]--);
  let f = 1.5;
  show(n + " frac pre++", ++a[f]);
  show(n + " frac read", a[f]);
  for (let i = 0; i < 8; i++) a[i]--;
  for (let i = 7; i >= 0; i--) ++a[i];
  for (let i = 0; i < 8; i++) a[i]--;
  show(n + " length", a.length);
  dump(n + " final", a);
}
localFloat32();
function localFloat64(): void {
  const a = new Float64Array(8);
  const n = "local Float64";
  a[0] = 9007199254740992;
  show(n + " post++ at max", a[0]++);
  a[1] = -9007199254740992;
  show(n + " post-- at min", a[1]--);
  a[2] = 9007199254740992;
  show(n + " pre++ at max", ++a[2]);
  a[3] = -9007199254740992;
  show(n + " pre-- at min", --a[3]);
  a[4] = 1.5;
  a[4]++;
  a[5] = NaN;
  show(n + " NaN post++", a[5]++);
  show(n + " NaN pre--", --a[5]);
  let k = 100;
  show(n + " oob post++", a[k]++);
  show(n + " oob pre--", --a[k]);
  k = -1;
  show(n + " neg post--", a[k]--);
  let f = 1.5;
  show(n + " frac pre++", ++a[f]);
  show(n + " frac read", a[f]);
  for (let i = 0; i < 8; i++) a[i]--;
  for (let i = 7; i >= 0; i--) ++a[i];
  for (let i = 0; i < 8; i++) a[i]--;
  show(n + " length", a.length);
  dump(n + " final", a);
}
localFloat64();
function localFloat16(): void {
  const a = new Float16Array(8);
  const n = "local Float16";
  a[0] = 2048;
  show(n + " post++ at max", a[0]++);
  a[1] = -65504;
  show(n + " post-- at min", a[1]--);
  a[2] = 2048;
  show(n + " pre++ at max", ++a[2]);
  a[3] = -65504;
  show(n + " pre-- at min", --a[3]);
  a[4] = 1.5;
  a[4]++;
  a[5] = NaN;
  show(n + " NaN post++", a[5]++);
  show(n + " NaN pre--", --a[5]);
  let k = 100;
  show(n + " oob post++", a[k]++);
  show(n + " oob pre--", --a[k]);
  k = -1;
  show(n + " neg post--", a[k]--);
  let f = 1.5;
  show(n + " frac pre++", ++a[f]);
  show(n + " frac read", a[f]);
  for (let i = 0; i < 8; i++) a[i]--;
  for (let i = 7; i >= 0; i--) ++a[i];
  for (let i = 0; i < 8; i++) a[i]--;
  show(n + " length", a.length);
  dump(n + " final", a);
}
localFloat16();

// ---- statically known locals (the specialized-entry shape) ----
function localKinds(): void {
  const i8 = new Int8Array(2);
  i8[0] = 127;
  i8[0]++;
  i8[1]--;
  dump("local Int8", i8);
  const c = new Uint8ClampedArray(4);
  c[0] = 255;
  c[0]++;
  c[1]--;
  c[2] = 2;
  c[2] += 0.5; // 2.5 rounds half to even: 2
  c[3] = 3;
  c[3] += 0.5; // 3.5 rounds half to even: 4
  dump("local Uint8Clamped", c);
  const i32 = new Int32Array(3);
  i32[0] = 2147483647;
  const r = ++i32[0];
  show("local Int32 ++max value", r);
  i32[1] = -2147483648;
  const q = i32[1]--;
  show("local Int32 min-- value", q);
  dump("local Int32", i32);
  const u32 = new Uint32Array(2);
  u32[0]--;
  u32[1] = 4294967295;
  u32[1]++;
  dump("local Uint32", u32);
  const f64 = new Float64Array(3);
  f64[0] = -0;
  show("local Float64 -0 post++", f64[0]++);
  f64[1] = 1;
  show("local Float64 pre-- to zero", --f64[1]);
  show("local Float64 zero sign", Object.is(f64[1], 0));
  f64[2] = 0.1;
  f64[2]++;
  dump("local Float64", f64);
  const f32 = new Float32Array(1);
  f32[0] = 16777216;
  f32[0]++; // 16777217 is not a float32: rounds back to 16777216
  dump("local Float32", f32);
}
localKinds();

// ---- BigInt kinds keep BigInt arithmetic ----
function bigKinds(): void {
  const b = new BigInt64Array(3);
  b[0] = 9223372036854775807n;
  show("BigInt64 post++ value", b[0]++);
  b[1]--;
  show("BigInt64 pre-- value", --b[1]);
  b[2] += 5n;
  b[2] *= 3n;
  dump("BigInt64", b);
  const u = new BigUint64Array(2);
  u[0]--;
  u[1] = 7n;
  u[1] <<= 2n;
  dump("BigUint64", u);
  let threw = "no";
  try {
    b[0] += 1 as unknown as bigint;
  } catch (e) {
    threw = (e as Error).constructor.name;
  }
  console.log("BigInt64 += Number throws", threw);
}
bigKinds();

// ---- a fannkuch-shaped countdown ----
function countdown(n: number): number {
  const count = new Int32Array(n);
  for (let r = 0; r < n; r++) count[r] = r;
  let steps = 0;
  let r = n - 1;
  while (r > 0) {
    count[r]--;
    steps++;
    if (count[r] > 0) continue;
    r--;
  }
  return steps;
}
show("countdown steps", countdown(12));

// ---- receiver and index are evaluated exactly once ----
function once(): void {
  const t = new Int16Array(4);
  let i = 0;
  let objCalls = 0;
  const get = (): Int16Array => {
    objCalls++;
    return t;
  };
  get()[i++]++;
  get()[i++] += 10;
  ++get()[i++];
  show("once objCalls", objCalls);
  show("once i", i);
  dump("once", t);
}
once();

// ---- a numeric getter on the prototype is never consulted ----
function protoGetter(): void {
  let hits = 0;
  Object.defineProperty(Int32Array.prototype, "20", {
    configurable: true,
    get() {
      hits++;
      return 5;
    },
    set(_v: unknown) {
      hits++;
    },
  });
  const t = new Int32Array(4);
  show("proto oob post++", t[20]++);
  show("proto oob pre--", --t[20]);
  t[20] += 1;
  t[2]++;
  show("proto hits", hits);
  dump("proto", t);
  delete (Int32Array.prototype as unknown as Record<string, unknown>)["20"];
}
protoGetter();

// ---- a typed parameter that receives something else at runtime ----
function bump(a: Int32Array, i: number): number {
  a[i]++;
  return a[i]--;
}
const liar1 = ["5", "x"] as unknown as Int32Array;
show("lying string array bump", bump(liar1, 0));
show("lying string array elem", (liar1 as unknown as unknown[])[0]);
show("lying string array x", bump(liar1, 1));
const liar2 = new BigInt64Array(2) as unknown as Int32Array;
show("lying BigInt64 bump", bump(liar2, 0));
dump("lying BigInt64", liar2 as unknown as BigInt64Array);
const liar3 = { 0: { valueOf: () => 41 } } as unknown as Int32Array;
show("lying object bump", bump(liar3, 0));
const view = new Int32Array(new ArrayBuffer(16), 4, 2);
view[1] = 9;
show("view bump", bump(view, 1));
dump("view", view);

// ---- loops over every element ----
function loops(): void {
  const f = new Float64Array(5);
  const u8 = new Uint8Array(5);
  const c = new Uint8ClampedArray(5);
  for (let k = 0; k < 300; k++) {
    for (let j = 0; j < 5; j++) {
      f[j] += j * 0.5;
      u8[j]++;
      c[j] += 1;
      f[j]--;
    }
  }
  dump("loop Float64", f);
  dump("loop Uint8", u8);
  dump("loop Uint8Clamped", c);
}
loops();
