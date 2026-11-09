// A wasm f64/f32 whose bits equal a NaN-box tag pattern must reach JS as NaN:
// export results, a mutable global, and arguments of an imported JS function.
// The module reinterprets i32 bits as floats:
//   f64of(hi, lo) -> f64   f32of(bits) -> f32   setg(hi, lo) sets global g
//   call64(hi, lo) / call32(bits) pass the float to env.s64 / env.s32.
const bytes = new Uint8Array([0,97,115,109,1,0,0,0,1,29,6,96,2,127,127,1,124,96,1,127,1,125,96,1,124,0,96,1,125,0,96,2,127,127,0,96,1,127,0,2,21,2,3,101,110,118,3,115,54,52,0,2,3,101,110,118,3,115,51,50,0,3,3,6,5,0,1,4,5,4,6,13,1,124,1,68,0,0,0,0,0,0,0,0,11,7,46,6,5,102,54,52,111,102,0,2,5,102,51,50,111,102,0,3,6,99,97,108,108,54,52,0,4,6,99,97,108,108,51,50,0,5,4,115,101,116,103,0,6,1,103,3,0,10,52,5,13,0,32,0,173,66,32,134,32,1,173,132,191,11,5,0,32,0,190,11,10,0,32,0,32,1,16,2,16,0,11,8,0,32,0,16,3,16,1,11,10,0,32,0,32,1,16,2,36,0,11]);

let seen: any = 0;
const imports = { env: { s64: (x: any) => { seen = x; }, s32: (x: any) => { seen = x; } } };
const exp: any = new WebAssembly.Instance(new WebAssembly.Module(bytes), imports).exports;

function show(label: string, x: any): void {
  console.log(label, typeof x, x === 5, Number.isNaN(x), Object.is(x, NaN), String(x));
}

// [hi, lo] u32 halves of the f64 pattern; heap-forging patterns last.
const F64: Array<[string, number, number]> = [
  ["quiet-payload", 0x7ff80000, 0x00000001],
  ["int32-5", 0x7ffe0000, 0x00000005],
  ["undefined", 0x7ffc0000, 0x00000001],
  ["null", 0x7ffc0000, 0x00000002],
  ["true", 0x7ffc0000, 0x00000004],
  ["neg-payload", 0xfffe0000, 0x00000005],
  ["handle", 0xfff90000, 0x00000001],
  ["pointer", 0x7ffd0000, 0x00000000],
  ["string", 0x7fff0000, 0x00000000],
];
// f32 NaN payloads widen to these f64 top halves: 0x7FFE, 0x7FFC, 0xFFF9, 0x7FFD, 0x7FFF.
const F32: Array<[string, number]> = [
  ["f32-int32", 0x7ff00005],
  ["f32-undefined-band", 0x7fe00000],
  ["f32-handle", 0xffc80000],
  ["f32-pointer", 0x7fe80000],
  ["f32-string", 0x7ff80000],
];

for (const [name, hi, lo] of F64) {
  show("result " + name, exp.f64of(hi | 0, lo | 0));
  exp.setg(hi | 0, lo | 0);
  show("global " + name, exp.g.value);
  seen = 0;
  exp.call64(hi | 0, lo | 0);
  show("import-arg " + name, seen);
}
for (const [name, bits] of F32) {
  show("result " + name, exp.f32of(bits | 0));
  seen = 0;
  exp.call32(bits | 0);
  show("import-arg " + name, seen);
}
