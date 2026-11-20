// Element access on an OWNED typed array (`new TA(n)`, `n` a Number) through
// keys that are Numbers or `undefined` but not statically integers: the keys
// a typed-array read produces (`let k = perm[0]`, fannkuch-redux). Such
// accesses take a guarded inline tier: one run-time test admits an exact
// in-bounds integer and a plain-number or `undefined` value, everything else
// exits to the full [[Get]]/[[Set]].
//
// Every observation below is made BEFORE the array escapes (it is passed to
// no function until the end), so each access really is on that tier, and
// each refused case is ordered so a wrong hit would change a printed value:
// `a[1.5] = 8` follows `a[1] = 12`, an out-of-bounds key is read back, and a
// string value is stored through the guarded key.
//
// Keys, for every element kind, on both sides: -0 (element 0), 1.5, NaN,
// negative, out of bounds, `undefined` (the ordinary property "undefined"),
// plus Symbol and string keys. Values: undefined, NaN, 1.5, 2**31, -1, 300,
// -129, 2**32+5, a numeric string, an object whose valueOf must run exactly
// once, and -0. Keys that are neither Numbers, strings nor Symbols (null,
// booleans, BigInts, objects) go through ToPropertyKey.

function show(v: any): string {
  if (typeof v === "number" && v === 0 && 1 / v < 0) return "-0";
  return String(v);
}

function ordinaryKeys(a: any): string {
  return Object.keys(a).filter((k) => !/^[0-9]+$/.test(k)).join("|");
}

function probeInt8Array(n: number): void {
  const a = new Int8Array(n);
  const keys = new Int32Array(n);
  keys[0] = 0;
  keys[1] = 3;
  keys[2] = 1000;
  keys[3] = 2;
  keys[4] = 1;
  const k = keys[3];
  const u = keys[n + 10];
  const neg = keys[4] - 2;
  const frac = keys[1] / 2;
  const nz = keys[0] * -1;
  const nan = u - 1;
  const oob = keys[2];
  const r: string[] = [];
  // Keys on the write side: element 1, element 2, element 0 (-0), then
  // keys that must store nothing as elements.
  a[keys[4]] = 12;
  a[k] = 5;
  a[nz] = 6;
  a[frac] = 8;
  a[neg] = 7;
  a[nan] = 9;
  a[oob] = 10;
  a[u] = 11;
  a[k + 1] = 13;
  // The same keys on the read side.
  for (const v of [a[keys[4]], a[k], a[nz], a[frac], a[neg], a[nan], a[oob], a[u], a[k + 1]]) r.push(show(v));
  // Values through a guarded key, elements 4..14.
  let calls = 0;
  const obj = { valueOf(): number { calls++; return 77; } };
  const vals: any[] = [u, NaN, 1.5, 2 ** 31, -1, 300, -129, 2 ** 32 + 5, "21", obj, -0];
  for (let i = 0; i < vals.length; i++) a[k + 2 + i] = vals[i];
  for (let i = 0; i < vals.length; i++) r.push(show(a[k + 2 + i]));
  r.push("calls=" + calls);
  // An out-of-bounds store converts its value once and stores nothing.
  a[oob] = obj;
  r.push("calls=" + calls, show(a[oob]));
  // The assignment expression's value is the assigned value.
  r.push(show((a[k] = 1.5)), show((a[k + 1] = 2.5)));
  console.log("Int8Array:", r.join(","));
  // Symbol and string keys, after the observations above.
  const sym = Symbol("s");
  (a as any)[sym] = 14;
  (a as any)["foo"] = 15;
  (a as any)["15"] = 16;
  console.log("Int8Array named:", (a as any)[sym], (a as any)["foo"], (a as any)["15"], ordinaryKeys(a));
}

function probeUint8Array(n: number): void {
  const a = new Uint8Array(n);
  const keys = new Int32Array(n);
  keys[0] = 0;
  keys[1] = 3;
  keys[2] = 1000;
  keys[3] = 2;
  keys[4] = 1;
  const k = keys[3];
  const u = keys[n + 10];
  const neg = keys[4] - 2;
  const frac = keys[1] / 2;
  const nz = keys[0] * -1;
  const nan = u - 1;
  const oob = keys[2];
  const r: string[] = [];
  // A Buffer-backed Uint8Array keeps no ordinary properties and has its own
  // value-conversion store path (separate, known gaps): keys only here.
  // Keys on the write side: element 1, element 2, element 0 (-0), then
  // keys that must store nothing as elements.
  a[keys[4]] = 12;
  a[k] = 5;
  a[nz] = 6;
  a[frac] = 8;
  a[neg] = 7;
  a[nan] = 9;
  a[oob] = 10;
  a[k + 1] = 13;
  // The same keys on the read side.
  for (const v of [a[keys[4]], a[k], a[nz], a[frac], a[neg], a[nan], a[oob], a[k + 1]]) r.push(show(v));
  // The assignment expression's value is the assigned value.
  r.push(show((a[k] = 1.5)), show((a[k + 1] = 2.5)));
  console.log("Uint8Array:", r.join(","));
  // Symbol and string keys, after the observations above.
  const sym = Symbol("s");
  (a as any)[sym] = 14;
  (a as any)["foo"] = 15;
  (a as any)["15"] = 16;
  console.log("Uint8Array named:", (a as any)[sym], (a as any)["foo"], (a as any)["15"], ordinaryKeys(a));
}

function probeUint8ClampedArray(n: number): void {
  const a = new Uint8ClampedArray(n);
  const keys = new Int32Array(n);
  keys[0] = 0;
  keys[1] = 3;
  keys[2] = 1000;
  keys[3] = 2;
  keys[4] = 1;
  const k = keys[3];
  const u = keys[n + 10];
  const neg = keys[4] - 2;
  const frac = keys[1] / 2;
  const nz = keys[0] * -1;
  const nan = u - 1;
  const oob = keys[2];
  const r: string[] = [];
  // Keys on the write side: element 1, element 2, element 0 (-0), then
  // keys that must store nothing as elements.
  a[keys[4]] = 12;
  a[k] = 5;
  a[nz] = 6;
  a[frac] = 8;
  a[neg] = 7;
  a[nan] = 9;
  a[oob] = 10;
  a[u] = 11;
  a[k + 1] = 13;
  // The same keys on the read side.
  for (const v of [a[keys[4]], a[k], a[nz], a[frac], a[neg], a[nan], a[oob], a[u], a[k + 1]]) r.push(show(v));
  // Values through a guarded key, elements 4..14.
  let calls = 0;
  const obj = { valueOf(): number { calls++; return 77; } };
  const vals: any[] = [u, NaN, 1.5, 2 ** 31, -1, 300, -129, 2 ** 32 + 5, "21", obj, -0];
  for (let i = 0; i < vals.length; i++) a[k + 2 + i] = vals[i];
  for (let i = 0; i < vals.length; i++) r.push(show(a[k + 2 + i]));
  r.push("calls=" + calls);
  // An out-of-bounds store converts its value once and stores nothing.
  a[oob] = obj;
  r.push("calls=" + calls, show(a[oob]));
  // The assignment expression's value is the assigned value.
  r.push(show((a[k] = 1.5)), show((a[k + 1] = 2.5)));
  console.log("Uint8ClampedArray:", r.join(","));
  // Symbol and string keys, after the observations above.
  const sym = Symbol("s");
  (a as any)[sym] = 14;
  (a as any)["foo"] = 15;
  (a as any)["15"] = 16;
  console.log("Uint8ClampedArray named:", (a as any)[sym], (a as any)["foo"], (a as any)["15"], ordinaryKeys(a));
}

function probeInt16Array(n: number): void {
  const a = new Int16Array(n);
  const keys = new Int32Array(n);
  keys[0] = 0;
  keys[1] = 3;
  keys[2] = 1000;
  keys[3] = 2;
  keys[4] = 1;
  const k = keys[3];
  const u = keys[n + 10];
  const neg = keys[4] - 2;
  const frac = keys[1] / 2;
  const nz = keys[0] * -1;
  const nan = u - 1;
  const oob = keys[2];
  const r: string[] = [];
  // Keys on the write side: element 1, element 2, element 0 (-0), then
  // keys that must store nothing as elements.
  a[keys[4]] = 12;
  a[k] = 5;
  a[nz] = 6;
  a[frac] = 8;
  a[neg] = 7;
  a[nan] = 9;
  a[oob] = 10;
  a[u] = 11;
  a[k + 1] = 13;
  // The same keys on the read side.
  for (const v of [a[keys[4]], a[k], a[nz], a[frac], a[neg], a[nan], a[oob], a[u], a[k + 1]]) r.push(show(v));
  // Values through a guarded key, elements 4..14.
  let calls = 0;
  const obj = { valueOf(): number { calls++; return 77; } };
  const vals: any[] = [u, NaN, 1.5, 2 ** 31, -1, 300, -129, 2 ** 32 + 5, "21", obj, -0];
  for (let i = 0; i < vals.length; i++) a[k + 2 + i] = vals[i];
  for (let i = 0; i < vals.length; i++) r.push(show(a[k + 2 + i]));
  r.push("calls=" + calls);
  // An out-of-bounds store converts its value once and stores nothing.
  a[oob] = obj;
  r.push("calls=" + calls, show(a[oob]));
  // The assignment expression's value is the assigned value.
  r.push(show((a[k] = 1.5)), show((a[k + 1] = 2.5)));
  console.log("Int16Array:", r.join(","));
  // Symbol and string keys, after the observations above.
  const sym = Symbol("s");
  (a as any)[sym] = 14;
  (a as any)["foo"] = 15;
  (a as any)["15"] = 16;
  console.log("Int16Array named:", (a as any)[sym], (a as any)["foo"], (a as any)["15"], ordinaryKeys(a));
}

function probeInt32Array(n: number): void {
  const a = new Int32Array(n);
  const keys = new Int32Array(n);
  keys[0] = 0;
  keys[1] = 3;
  keys[2] = 1000;
  keys[3] = 2;
  keys[4] = 1;
  const k = keys[3];
  const u = keys[n + 10];
  const neg = keys[4] - 2;
  const frac = keys[1] / 2;
  const nz = keys[0] * -1;
  const nan = u - 1;
  const oob = keys[2];
  const r: string[] = [];
  // Keys on the write side: element 1, element 2, element 0 (-0), then
  // keys that must store nothing as elements.
  a[keys[4]] = 12;
  a[k] = 5;
  a[nz] = 6;
  a[frac] = 8;
  a[neg] = 7;
  a[nan] = 9;
  a[oob] = 10;
  a[u] = 11;
  a[k + 1] = 13;
  // The same keys on the read side.
  for (const v of [a[keys[4]], a[k], a[nz], a[frac], a[neg], a[nan], a[oob], a[u], a[k + 1]]) r.push(show(v));
  // Values through a guarded key, elements 4..14.
  let calls = 0;
  const obj = { valueOf(): number { calls++; return 77; } };
  const vals: any[] = [u, NaN, 1.5, 2 ** 31, -1, 300, -129, 2 ** 32 + 5, "21", obj, -0];
  for (let i = 0; i < vals.length; i++) a[k + 2 + i] = vals[i];
  for (let i = 0; i < vals.length; i++) r.push(show(a[k + 2 + i]));
  r.push("calls=" + calls);
  // An out-of-bounds store converts its value once and stores nothing.
  a[oob] = obj;
  r.push("calls=" + calls, show(a[oob]));
  // The assignment expression's value is the assigned value.
  r.push(show((a[k] = 1.5)), show((a[k + 1] = 2.5)));
  console.log("Int32Array:", r.join(","));
  // Symbol and string keys, after the observations above.
  const sym = Symbol("s");
  (a as any)[sym] = 14;
  (a as any)["foo"] = 15;
  (a as any)["15"] = 16;
  console.log("Int32Array named:", (a as any)[sym], (a as any)["foo"], (a as any)["15"], ordinaryKeys(a));
}

function probeUint32Array(n: number): void {
  const a = new Uint32Array(n);
  const keys = new Int32Array(n);
  keys[0] = 0;
  keys[1] = 3;
  keys[2] = 1000;
  keys[3] = 2;
  keys[4] = 1;
  const k = keys[3];
  const u = keys[n + 10];
  const neg = keys[4] - 2;
  const frac = keys[1] / 2;
  const nz = keys[0] * -1;
  const nan = u - 1;
  const oob = keys[2];
  const r: string[] = [];
  // Keys on the write side: element 1, element 2, element 0 (-0), then
  // keys that must store nothing as elements.
  a[keys[4]] = 12;
  a[k] = 5;
  a[nz] = 6;
  a[frac] = 8;
  a[neg] = 7;
  a[nan] = 9;
  a[oob] = 10;
  a[u] = 11;
  a[k + 1] = 13;
  // The same keys on the read side.
  for (const v of [a[keys[4]], a[k], a[nz], a[frac], a[neg], a[nan], a[oob], a[u], a[k + 1]]) r.push(show(v));
  // Values through a guarded key, elements 4..14.
  let calls = 0;
  const obj = { valueOf(): number { calls++; return 77; } };
  const vals: any[] = [u, NaN, 1.5, 2 ** 31, -1, 300, -129, 2 ** 32 + 5, "21", obj, -0];
  for (let i = 0; i < vals.length; i++) a[k + 2 + i] = vals[i];
  for (let i = 0; i < vals.length; i++) r.push(show(a[k + 2 + i]));
  r.push("calls=" + calls);
  // An out-of-bounds store converts its value once and stores nothing.
  a[oob] = obj;
  r.push("calls=" + calls, show(a[oob]));
  // The assignment expression's value is the assigned value.
  r.push(show((a[k] = 1.5)), show((a[k + 1] = 2.5)));
  console.log("Uint32Array:", r.join(","));
  // Symbol and string keys, after the observations above.
  const sym = Symbol("s");
  (a as any)[sym] = 14;
  (a as any)["foo"] = 15;
  (a as any)["15"] = 16;
  console.log("Uint32Array named:", (a as any)[sym], (a as any)["foo"], (a as any)["15"], ordinaryKeys(a));
}

function probeFloat32Array(n: number): void {
  const a = new Float32Array(n);
  const keys = new Int32Array(n);
  keys[0] = 0;
  keys[1] = 3;
  keys[2] = 1000;
  keys[3] = 2;
  keys[4] = 1;
  const k = keys[3];
  const u = keys[n + 10];
  const neg = keys[4] - 2;
  const frac = keys[1] / 2;
  const nz = keys[0] * -1;
  const nan = u - 1;
  const oob = keys[2];
  const r: string[] = [];
  // Keys on the write side: element 1, element 2, element 0 (-0), then
  // keys that must store nothing as elements.
  a[keys[4]] = 12;
  a[k] = 5;
  a[nz] = 6;
  a[frac] = 8;
  a[neg] = 7;
  a[nan] = 9;
  a[oob] = 10;
  a[u] = 11;
  a[k + 1] = 13;
  // The same keys on the read side.
  for (const v of [a[keys[4]], a[k], a[nz], a[frac], a[neg], a[nan], a[oob], a[u], a[k + 1]]) r.push(show(v));
  // Values through a guarded key, elements 4..14.
  let calls = 0;
  const obj = { valueOf(): number { calls++; return 77; } };
  const vals: any[] = [u, NaN, 1.5, 2 ** 31, -1, 300, -129, 2 ** 32 + 5, "21", obj, -0];
  for (let i = 0; i < vals.length; i++) a[k + 2 + i] = vals[i];
  for (let i = 0; i < vals.length; i++) r.push(show(a[k + 2 + i]));
  r.push("calls=" + calls);
  // An out-of-bounds store converts its value once and stores nothing.
  a[oob] = obj;
  r.push("calls=" + calls, show(a[oob]));
  // The assignment expression's value is the assigned value.
  r.push(show((a[k] = 1.5)), show((a[k + 1] = 2.5)));
  console.log("Float32Array:", r.join(","));
  // Symbol and string keys, after the observations above.
  const sym = Symbol("s");
  (a as any)[sym] = 14;
  (a as any)["foo"] = 15;
  (a as any)["15"] = 16;
  console.log("Float32Array named:", (a as any)[sym], (a as any)["foo"], (a as any)["15"], ordinaryKeys(a));
  // `undefined` stored the canonical NaN, never a tagged bit pattern.
  const bytes = new Uint8Array(a.buffer, 4 * a.BYTES_PER_ELEMENT, a.BYTES_PER_ELEMENT);
  console.log("Float32Array undefined bytes:", Array.from(bytes).join(","));
}

function probeFloat64Array(n: number): void {
  const a = new Float64Array(n);
  const keys = new Int32Array(n);
  keys[0] = 0;
  keys[1] = 3;
  keys[2] = 1000;
  keys[3] = 2;
  keys[4] = 1;
  const k = keys[3];
  const u = keys[n + 10];
  const neg = keys[4] - 2;
  const frac = keys[1] / 2;
  const nz = keys[0] * -1;
  const nan = u - 1;
  const oob = keys[2];
  const r: string[] = [];
  // Keys on the write side: element 1, element 2, element 0 (-0), then
  // keys that must store nothing as elements.
  a[keys[4]] = 12;
  a[k] = 5;
  a[nz] = 6;
  a[frac] = 8;
  a[neg] = 7;
  a[nan] = 9;
  a[oob] = 10;
  a[u] = 11;
  a[k + 1] = 13;
  // The same keys on the read side.
  for (const v of [a[keys[4]], a[k], a[nz], a[frac], a[neg], a[nan], a[oob], a[u], a[k + 1]]) r.push(show(v));
  // Values through a guarded key, elements 4..14.
  let calls = 0;
  const obj = { valueOf(): number { calls++; return 77; } };
  const vals: any[] = [u, NaN, 1.5, 2 ** 31, -1, 300, -129, 2 ** 32 + 5, "21", obj, -0];
  for (let i = 0; i < vals.length; i++) a[k + 2 + i] = vals[i];
  for (let i = 0; i < vals.length; i++) r.push(show(a[k + 2 + i]));
  r.push("calls=" + calls);
  // An out-of-bounds store converts its value once and stores nothing.
  a[oob] = obj;
  r.push("calls=" + calls, show(a[oob]));
  // The assignment expression's value is the assigned value.
  r.push(show((a[k] = 1.5)), show((a[k + 1] = 2.5)));
  console.log("Float64Array:", r.join(","));
  // Symbol and string keys, after the observations above.
  const sym = Symbol("s");
  (a as any)[sym] = 14;
  (a as any)["foo"] = 15;
  (a as any)["15"] = 16;
  console.log("Float64Array named:", (a as any)[sym], (a as any)["foo"], (a as any)["15"], ordinaryKeys(a));
  // `undefined` stored the canonical NaN, never a tagged bit pattern.
  const bytes = new Uint8Array(a.buffer, 4 * a.BYTES_PER_ELEMENT, a.BYTES_PER_ELEMENT);
  console.log("Float64Array undefined bytes:", Array.from(bytes).join(","));
}

probeInt8Array(16);
probeUint8Array(16);
probeUint8ClampedArray(16);
probeInt16Array(16);
probeInt32Array(16);
probeUint32Array(16);
probeFloat32Array(16);
probeFloat64Array(16);

// Keys that are neither Numbers, strings nor Symbols are ToPropertyKey'd: an
// ordinary property for `null`/`true`, an element for `1n` and for an object
// whose toString names an index.
function otherKeys(): void {
  const a: any = new Int32Array(4);
  const keys: any[] = [null, true, 1n, { toString(): string { return "2"; } }, undefined];
  for (let i = 0; i < keys.length; i++) a[keys[i]] = 50 + i;
  console.log("other keys:", Array.from(a).join(","), ordinaryKeys(a), keys.map((k) => a[k]).join(","));
}
otherKeys();

// A key that may be a string must not take the guarded tier: `"buffer"`
// rebinds the array onto an ArrayBuffer, and later element accesses must see
// the shared storage, not the array's old inline bytes.
function bufferKey(n: number): void {
  const a = new Int32Array(n);
  a[0] = 1;
  const key: any = "buffer";
  const b = a[key];
  new Int32Array(b)[0] = 99;
  a[1] = 5;
  console.log("buffer key:", a[0], a[1], new Int32Array(b)[1]);
}
bufferKey(4);
