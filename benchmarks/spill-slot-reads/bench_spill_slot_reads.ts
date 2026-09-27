// S5 slope fixture: instructions per property read of a SPILL-located key,
// against the same loop reading an INLINE key. Run with N = 1000000 and
// 5000000 and take (I(5M) - I(1M)) / 4M per mode.
//   node --experimental-strip-types bench_spill_slot_reads.ts <N> <mode>
// mode: spill | inline | undef | mega
// Receivers are parameters, rotated through an array, so the read is neither
// refined by the binding nor hoisted out of the loop.

// The keys are added by a SEPARATE function: added in the literal's own
// function, the compiler sizes the literal for them and they are inline.
function tail(o: any, i: number): any {
  o.c = i; // spill 2
  o.d = i + 1; // spill 3
  o.u = undefined; // spill 4
  return o;
}
function tailShape(o: any, i: number, s: number): any {
  for (let k = 0; k < s; k++) o["q" + s + "_" + k] = k;
  o.d = i + 1;
  return o;
}

function readSpill(o: any): number {
  return o.d;
}
function readInline(o: any): number {
  return o.b;
}
function readUndef(o: any): any {
  return o.u;
}

function run(n: number, mode: string): number {
  const objs: any[] = [];
  // The literal is built HERE (two inline slots) and grown by a helper. (Built
  // inside a small per-object factory instead, every receiver got a shape of
  // its own on current main: a separate, pre-existing issue.)
  for (let i = 0; i < 1024; i++) {
    objs.push(mode === "mega" ? tailShape({ a: i, b: i }, i, i % 16) : tail({ a: i, b: i }, i));
  }
  let s = 0;
  if (mode === "inline") {
    for (let i = 0; i < n; i++) s += readInline(objs[i & 1023]);
  } else if (mode === "undef") {
    for (let i = 0; i < n; i++) if (readUndef(objs[i & 1023]) === undefined) s++;
  } else {
    for (let i = 0; i < n; i++) s += readSpill(objs[i & 1023]);
  }
  return s;
}

const n = Number(process.argv[2] || 1000000);
const mode = process.argv[3] || "spill";
console.log(mode, n, run(n, mode));
