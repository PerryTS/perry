// The element-shape fast clone's element binding (`const o = a[i]`) is
// virtual: it binds no slot, and every `o.field` read lowers through the loop
// fact. A class the whole-program facts prove (no subclass, fields born at
// fixed slots) let the wpfacts zero-guard read claim `o.x` first and lower the
// unbound `o`: "Cannot read properties of undefined (reading 'x')" on the
// first iteration (competitive C/object-get-1).

class P3 {
  x: number;
  y: number;
  z: number;
  tag: number;
  constructor(x: number, y: number, z: number, tag: number) {
    this.x = x;
    this.y = y;
    this.z = z;
    this.tag = tag;
  }
}

const insts: P3[] = [];
for (let i = 0; i < 1000; i++) insts.push(new P3(i, i * 2, i * 3, i & 7));

function sumInsts(a: P3[]): number {
  let s = 0;
  for (let i = 0; i < a.length; i++) {
    const o = a[i];
    s += o.x + o.y + o.z + o.tag;
  }
  return s;
}

function sumX(a: P3[]): number {
  let s = 0;
  for (let i = 0; i < a.length; i++) {
    const o = a[i];
    s += o.x;
  }
  return s;
}

let total = 0;
for (let r = 0; r < 50; r++) total = (total + sumInsts(insts)) % 1_000_000_007;
console.log("sumInsts", sumInsts(insts), total);
console.log("sumX", sumX(insts), sumX([new P3(1, 2, 3, 4), new P3(5, 6, 7, 8)]), sumX([]));
