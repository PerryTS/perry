// Element receivers (decision 67): `const bi = bs[i]` inside a nested loop
// forms an array loop region with a nested body region (the nbody shape).
// Every case breaks one fact the region proved (the array, an element's
// shape, the bound, a prototype, a key count) in the middle of a loop, or
// starts with elements the compile-time guess does not fit, and must print
// exactly what node prints. The cases are generated from one kernel; they
// differ only in the mutation spliced in at the marked point. Each case has
// its own class, so a case that reshapes instances cannot change what the
// next case's loops see.

class Body2 {
  mass: number; vz: number; vy: number; vx: number; z: number; y: number; x: number;
  constructor(s: number) {
    this.mass = 1 + s; this.vz = 0.3 / s; this.vy = 0.2; this.vx = 0.1 * s; this.z = s * 3; this.y = s * 2; this.x = s;
  }
}
class Body3 {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number; tag: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s; this.tag = 5;
  }
}
// Accessor descriptors come from classes: a getter named as a plain value
// would need a function wrapper that cannot be reached across codegen units.
class OneHalf { get half(): number { return 1; } }
class TwoMass { get mass(): number { return 2; } }
class ElemTwo { get 2(): any { return b2ref; } }
class MyArr extends Array<any> {}

let b2ref: any;
let step = 0;
let sink = 0;

function defAcc(o: any): void {
  Object.defineProperty(o, "mass", Object.getOwnPropertyDescriptor(TwoMass.prototype, "mass")!);
}
function readOnly(o: any): void {
  Object.defineProperty(o, "vx", { writable: false });
}
function put(bs: any[], k: number, v: number): void { bs[k] = new Body3(v); }
function reshape(b: any): void { b.extra = 3; delete b.vy; }

function mk(kind: string, C: any): any[] {
  const bs: any[] = [];
  for (let i = 0; i < 6; i++) {
    const s = i + 1;
    if (kind === "mixed") bs.push(i % 3 === 0 ? new C(s) : i % 3 === 1 ? new Body2(s) : new Body3(s));
    else bs.push(new C(s));
  }
  if (kind === "extra") bs[2].extra = 1;
  if (kind === "alias") { bs[3] = bs[1]; bs[5] = bs[1]; }
  if (kind === "holes") { delete bs[2]; delete bs[4]; }
  if (kind === "arraylike") {
    const o: any = { length: bs.length };
    for (let i = 0; i < bs.length; i++) o[i] = bs[i];
    return o;
  }
  if (kind === "accessor") {
    b2ref = bs[2];
    Object.defineProperty(bs, 2, Object.getOwnPropertyDescriptor(ElemTwo.prototype, "2")!);
  }
  if (kind === "subclass") {
    const a = new MyArr();
    for (let i = 0; i < bs.length; i++) a.push(bs[i]);
    return a;
  }
  return bs;
}

function sum(bs: any[]): string {
  let s = 0;
  for (let i = 0; i < bs.length; i++) {
    const b = bs[i];
    if (b === undefined) { s += 1000; continue; }
    s += b.x + b.y + b.z + b.vx + b.vy + b.vz;
  }
  return s.toFixed(6);
}

function keys(bs: any[]): string {
  const out: string[] = [];
  for (let i = 0; i < bs.length; i++) out.push(bs[i] === undefined ? "-" : Object.keys(bs[i]).length + "");
  return out.join(",");
}

function report(name: string, bs: any[], errs: number): void {
  console.log(name, sum(bs), bs.length, keys(bs), errs);
}

class B_alias_elem_pre {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_alias_elem_pre(bs: B_alias_elem_pre[], al: B_alias_elem_pre[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      if (step === 20 && i === 1 && j === 3) { bsa[j] = bs[i]; }
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_alias_elem_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_alias_elem_post(bs: B_alias_elem_post[], al: B_alias_elem_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { bsa[j] = bs[i]; }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_alias_array_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_alias_array_post(bs: B_alias_array_post[], al: B_alias_array_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { ala[(j + 1) % n] = al[0]; }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_alias_start_pre {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_alias_start_pre(bs: B_alias_start_pre[], al: B_alias_start_pre[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_shape_add_j_pre {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_shape_add_j_pre(bs: B_shape_add_j_pre[], al: B_shape_add_j_pre[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      if (step === 20 && i === 1 && j === 3) { (bj as any).extra = 1; }
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_shape_add_j_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_shape_add_j_post(bs: B_shape_add_j_post[], al: B_shape_add_j_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { (bj as any).extra = 1; }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_shape_delete_j_pre {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_shape_delete_j_pre(bs: B_shape_delete_j_pre[], al: B_shape_delete_j_pre[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      if (step === 20 && i === 1 && j === 3) { delete (bj as any).vz; }
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_shape_delete_j_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_shape_delete_j_post(bs: B_shape_delete_j_post[], al: B_shape_delete_j_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { delete (bj as any).vz; }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_shape_add_i_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_shape_add_i_post(bs: B_shape_add_i_post[], al: B_shape_add_i_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { (bi as any).extra = 2; }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_store_other_order_pre {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_store_other_order_pre(bs: B_store_other_order_pre[], al: B_store_other_order_pre[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      if (step === 20 && i === 1 && j === 3) { bsa[n - 1] = new Body2(9); }
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_store_other_order_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_store_other_order_post(bs: B_store_other_order_post[], al: B_store_other_order_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { bsa[n - 1] = new Body2(9); }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_store_other_next_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_store_other_next_post(bs: B_store_other_next_post[], al: B_store_other_next_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { bsa[j + 1] = new Body3(7); }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_callee_store_pre {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_callee_store_pre(bs: B_callee_store_pre[], al: B_callee_store_pre[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      if (step === 20 && i === 1 && j === 3) { put(bs, n - 1, 3); }
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_callee_store_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_callee_store_post(bs: B_callee_store_post[], al: B_callee_store_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { put(bs, n - 1, 3); }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_callee_reshape_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_callee_reshape_post(bs: B_callee_reshape_post[], al: B_callee_reshape_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { reshape(bs[n - 1]); }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_hole_delete_pre {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_hole_delete_pre(bs: B_hole_delete_pre[], al: B_hole_delete_pre[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      if (step === 20 && i === 1 && j === 3) { delete bsa[n - 1]; }
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_hole_delete_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_hole_delete_post(bs: B_hole_delete_post[], al: B_hole_delete_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { delete bsa[n - 1]; }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_holes_start_pre {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_holes_start_pre(bs: B_holes_start_pre[], al: B_holes_start_pre[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_len_push_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_len_push_post(bs: B_len_push_post[], al: B_len_push_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { bs.push(new Body2(77) as any); }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_len_push_live_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_len_push_live_post(bs: B_len_push_live_post[], al: B_len_push_live_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < bs.length; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { bs.push(new Body2(77) as any); }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_len_pop_pre {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_len_pop_pre(bs: B_len_pop_pre[], al: B_len_pop_pre[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      if (step === 20 && i === 1 && j === 3) { bs.pop(); }
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_len_pop_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_len_pop_post(bs: B_len_pop_post[], al: B_len_pop_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { bs.pop(); }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_len_shrink_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_len_shrink_post(bs: B_len_shrink_post[], al: B_len_shrink_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { bs.length = n - 2; }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_len_grow_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_len_grow_post(bs: B_len_grow_post[], al: B_len_grow_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { bs.length = n + 5; }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_proto_set_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_proto_set_post(bs: B_proto_set_post[], al: B_proto_set_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { Object.setPrototypeOf(bj, Body2.prototype); }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_proto_set_i_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_proto_set_i_post(bs: B_proto_set_i_post[], al: B_proto_set_i_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { Object.setPrototypeOf(bi, Body3.prototype); }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_mixed_start_pre {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_mixed_start_pre(bs: B_mixed_start_pre[], al: B_mixed_start_pre[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_extra_start_pre {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_extra_start_pre(bs: B_extra_start_pre[], al: B_extra_start_pre[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_define_accessor_i_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_define_accessor_i_post(bs: B_define_accessor_i_post[], al: B_define_accessor_i_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { defAcc(bi); }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_define_accessor_j_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_define_accessor_j_post(bs: B_define_accessor_j_post[], al: B_define_accessor_j_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { defAcc(bj); }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_freeze_i_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_freeze_i_post(bs: B_freeze_i_post[], al: B_freeze_i_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { Object.freeze(bi); }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_readonly_j_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_readonly_j_post(bs: B_readonly_j_post[], al: B_readonly_j_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { readOnly(bj); }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_store_nonnumber_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_store_nonnumber_post(bs: B_store_nonnumber_post[], al: B_store_nonnumber_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { bi.vx = (j === 3 ? "s" : 0.5) as any; }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_accessor_via_i_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_accessor_via_i_post(bs: B_accessor_via_i_post[], al: B_accessor_via_i_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { defAcc(bs[i]); }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_accessor_via_j_pre {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_accessor_via_j_pre(bs: B_accessor_via_j_pre[], al: B_accessor_via_j_pre[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      if (step === 20 && i === 1 && j === 3) { defAcc(bs[j]); }
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_accessor_via_j_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_accessor_via_j_post(bs: B_accessor_via_j_post[], al: B_accessor_via_j_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { defAcc(bs[j]); }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_freeze_via_i_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_freeze_via_i_post(bs: B_freeze_via_i_post[], al: B_freeze_via_i_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { Object.freeze(bs[i]); }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_readonly_via_i_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_readonly_via_i_post(bs: B_readonly_via_i_post[], al: B_readonly_via_i_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { readOnly(bs[i]); }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_readonly_via_j_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_readonly_via_j_post(bs: B_readonly_via_j_post[], al: B_readonly_via_j_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { readOnly(bs[j]); }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_delete_via_i_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_delete_via_i_post(bs: B_delete_via_i_post[], al: B_delete_via_i_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { delete bsa[i].vz; }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_reshape_via_i_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_reshape_via_i_post(bs: B_reshape_via_i_post[], al: B_reshape_via_i_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { reshape(bs[i]); }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_reshape_via_j_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_reshape_via_j_post(bs: B_reshape_via_j_post[], al: B_reshape_via_j_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { reshape(bs[j]); }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_add_via_i_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_add_via_i_post(bs: B_add_via_i_post[], al: B_add_via_i_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { bsa[i].extra = 5; }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_array_arraylike_pre {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_array_arraylike_pre(bs: B_array_arraylike_pre[], al: B_array_arraylike_pre[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_array_accessor_pre {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_array_accessor_pre(bs: B_array_accessor_pre[], al: B_array_accessor_pre[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_array_subclass_pre {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_array_subclass_pre(bs: B_array_subclass_pre[], al: B_array_subclass_pre[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_extra_mid_static_post {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_extra_mid_static_post(bs: B_extra_mid_static_post[], al: B_extra_mid_static_post[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      if (step === 20 && i === 1 && j === 3) { bsa[n - 1].extra = 1; }
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_accessor {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function redefineHalf(): void {
  Object.defineProperty(B_accessor.prototype, "half", Object.getOwnPropertyDescriptor(OneHalf.prototype, "half")!);
}
function k_accessor(bs: B_accessor[], al: B_accessor[], dt: number, n: number): void {
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      if (step === 20 && i === 1 && j === 3) redefineHalf();
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const mag = dt / (dx * dx + dy * dy + dz * dz + 0.5);
      bi.vx -= dx * bj.half * mag; bj.vx += dx * bi.half * mag;
      bi.vy -= dy * mag; bj.vy += dy * mag; bi.vz -= dz * mag; bj.vz += dz * mag;
    }
  }
}
class B_hole_skip {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_hole_skip(bs: B_hole_skip[], al: B_hole_skip[], dt: number, n: number): void {
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    if (bi === undefined) continue;
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      if (bj === undefined) continue;
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const mag = dt / (dx * dx + dy * dy + dz * dz + 0.5);
      bi.vx -= dx * bj.mass * mag; bj.vx += dx * bi.mass * mag;
      bi.vy -= dy * mag; bj.vy += dy * mag; bi.vz -= dz * mag; bj.vz += dz * mag;
    }
  }
}
class B_gc {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_gc(bs: B_gc[], al: B_gc[], dt: number, n: number): void {
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      const junk: number[][] = [];
      for (let q = 0; q < 20; q++) junk.push([q, i, j, step]);
      if (step % 5 === 0 && j === 3) { const big = new Array(2000); for (let q = 0; q < 2000; q++) big[q] = { q }; sink += big.length; }
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      sink += junk.length;
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_gc_mixed {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_gc_mixed(bs: B_gc_mixed[], al: B_gc_mixed[], dt: number, n: number): void {
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      const junk: number[][] = [];
      for (let q = 0; q < 20; q++) junk.push([q, i, j, step]);
      if (step % 5 === 0 && j === 3) { const big = new Array(2000); for (let q = 0; q < 2000; q++) big[q] = { q }; sink += big.length; }
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      sink += junk.length;
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_gc_grow {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_gc_grow(bs: B_gc_grow[], al: B_gc_grow[], dt: number, n: number): void {
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      if (step % 4 === 0 && i === 0 && j === 2) { for (let q = 0; q < 40; q++) bs.push(bs[q % n]); bs.length = n; }
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
    }
  }
}
class B_oob_bound_skip {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_oob_bound_skip(bs: B_oob_bound_skip[], al: B_oob_bound_skip[], dt: number, n: number): void {
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    if (bi === undefined) continue;
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      if (bj === undefined) continue;
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const mag = dt / (dx * dx + dy * dy + dz * dz + 0.5);
      bi.vx -= dx * bj.mass * mag; bj.vx += dx * bi.mass * mag;
      bi.vy -= dy * mag; bj.vy += dy * mag; bi.vz -= dz * mag; bj.vz += dz * mag;
    }
  }
}
class B_oob_bound_throw {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_oob_bound_throw(bs: B_oob_bound_throw[], al: B_oob_bound_throw[], dt: number, n: number): void {
  const bsa: any = bs;
  const ala: any = al;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      
      const dx = bi.x - bj.x, dy = bi.y - bj.y, dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz + 0.5;
      const mag = dt / (d2 * Math.sqrt(d2));
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag;
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag;
      
    }
  }
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz;
  }
}
class B_oob_far {
  x: number; y: number; z: number; vx: number; vy: number; vz: number; mass: number;
  constructor(s: number) {
    this.x = s; this.y = s * 2; this.z = s * 3; this.vx = 0.1 * s; this.vy = 0.2; this.vz = 0.3 / s; this.mass = 1 + s;
  }
  get half(): number { return this.mass * 0.5; }
}
function k_oob_far(bs: B_oob_far[], al: B_oob_far[], dt: number, n: number): void {
  let acc = 0;
  for (let i = 0; i < 1; i++) {
    const bi = bs[i];
    for (let j = 1; j < n; j++) {
      const bj = bs[j];
      if (bj === undefined) continue;
      bi.vx += bj.vy * 1e-9;
      acc += bj.x;
    }
  }
  sink += acc;
}
class Big31 {
  f0: number;
  f1: number;
  f2: number;
  f3: number;
  f4: number;
  f5: number;
  f6: number;
  f7: number;
  f8: number;
  f9: number;
  f10: number;
  f11: number;
  f12: number;
  f13: number;
  f14: number;
  f15: number;
  f16: number;
  f17: number;
  f18: number;
  f19: number;
  f20: number;
  f21: number;
  f22: number;
  f23: number;
  f24: number;
  f25: number;
  f26: number;
  f27: number;
  f28: number;
  f29: number;
  f30: number;
  constructor(s: number) {
    this.f0 = s + 0;
    this.f1 = s + 1;
    this.f2 = s + 2;
    this.f3 = s + 3;
    this.f4 = s + 4;
    this.f5 = s + 5;
    this.f6 = s + 6;
    this.f7 = s + 7;
    this.f8 = s + 8;
    this.f9 = s + 9;
    this.f10 = s + 10;
    this.f11 = s + 11;
    this.f12 = s + 12;
    this.f13 = s + 13;
    this.f14 = s + 14;
    this.f15 = s + 15;
    this.f16 = s + 16;
    this.f17 = s + 17;
    this.f18 = s + 18;
    this.f19 = s + 19;
    this.f20 = s + 20;
    this.f21 = s + 21;
    this.f22 = s + 22;
    this.f23 = s + 23;
    this.f24 = s + 24;
    this.f25 = s + 25;
    this.f26 = s + 26;
    this.f27 = s + 27;
    this.f28 = s + 28;
    this.f29 = s + 29;
    this.f30 = s + 30;
  }
}
function kbig31(bs: Big31[], n: number, grow: number): number {
  let acc = 0;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      if (grow === step && j === 2) (bj as any).late = 1;
      const d = bi.f0 - bj.f30;
      bi.f1 += d * 0.001; bj.f29 -= d * 0.001; bi.f30 += bj.f3 * 1e-4;
      acc += bi.f1 + bj.f29;
    }
  }
  return acc;
}
function runBig31(grow: number): void {
  const bs: Big31[] = [];
  for (let i = 0; i < 5; i++) bs.push(new Big31(i + 1));
  let acc = 0;
  for (step = 0; step < 40; step++) acc += kbig31(bs, bs.length, grow);
  console.log("big31_" + (grow < 0 ? "static" : "grows"), acc.toFixed(6), keys(bs));
}
class Big33 {
  f0: number;
  f1: number;
  f2: number;
  f3: number;
  f4: number;
  f5: number;
  f6: number;
  f7: number;
  f8: number;
  f9: number;
  f10: number;
  f11: number;
  f12: number;
  f13: number;
  f14: number;
  f15: number;
  f16: number;
  f17: number;
  f18: number;
  f19: number;
  f20: number;
  f21: number;
  f22: number;
  f23: number;
  f24: number;
  f25: number;
  f26: number;
  f27: number;
  f28: number;
  f29: number;
  f30: number;
  f31: number;
  f32: number;
  constructor(s: number) {
    this.f0 = s + 0;
    this.f1 = s + 1;
    this.f2 = s + 2;
    this.f3 = s + 3;
    this.f4 = s + 4;
    this.f5 = s + 5;
    this.f6 = s + 6;
    this.f7 = s + 7;
    this.f8 = s + 8;
    this.f9 = s + 9;
    this.f10 = s + 10;
    this.f11 = s + 11;
    this.f12 = s + 12;
    this.f13 = s + 13;
    this.f14 = s + 14;
    this.f15 = s + 15;
    this.f16 = s + 16;
    this.f17 = s + 17;
    this.f18 = s + 18;
    this.f19 = s + 19;
    this.f20 = s + 20;
    this.f21 = s + 21;
    this.f22 = s + 22;
    this.f23 = s + 23;
    this.f24 = s + 24;
    this.f25 = s + 25;
    this.f26 = s + 26;
    this.f27 = s + 27;
    this.f28 = s + 28;
    this.f29 = s + 29;
    this.f30 = s + 30;
    this.f31 = s + 31;
    this.f32 = s + 32;
  }
}
function kbig33(bs: Big33[], n: number, grow: number): number {
  let acc = 0;
  for (let i = 0; i < n; i++) {
    const bi = bs[i];
    for (let j = i + 1; j < n; j++) {
      const bj = bs[j];
      if (grow === step && j === 2) (bj as any).late = 1;
      const d = bi.f0 - bj.f32;
      bi.f1 += d * 0.001; bj.f31 -= d * 0.001; bi.f32 += bj.f3 * 1e-4;
      acc += bi.f1 + bj.f31;
    }
  }
  return acc;
}
function runBig33(grow: number): void {
  const bs: Big33[] = [];
  for (let i = 0; i < 5; i++) bs.push(new Big33(i + 1));
  let acc = 0;
  for (step = 0; step < 40; step++) acc += kbig33(bs, bs.length, grow);
  console.log("big33_" + (grow < 0 ? "static" : "grows"), acc.toFixed(6), keys(bs));
}
{ const bs = mk("plain", B_alias_elem_pre); let errs = 0; for (step = 0; step < 40; step++) { try { k_alias_elem_pre(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("alias_elem_pre", bs, errs); }
{ const bs = mk("plain", B_alias_elem_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_alias_elem_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("alias_elem_post", bs, errs); }
{ const bs = mk("plain", B_alias_array_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_alias_array_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("alias_array_post", bs, errs); }
{ const bs = mk("alias", B_alias_start_pre); let errs = 0; for (step = 0; step < 40; step++) { try { k_alias_start_pre(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("alias_start_pre", bs, errs); }
{ const bs = mk("plain", B_shape_add_j_pre); let errs = 0; for (step = 0; step < 40; step++) { try { k_shape_add_j_pre(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("shape_add_j_pre", bs, errs); }
{ const bs = mk("plain", B_shape_add_j_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_shape_add_j_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("shape_add_j_post", bs, errs); }
{ const bs = mk("plain", B_shape_delete_j_pre); let errs = 0; for (step = 0; step < 40; step++) { try { k_shape_delete_j_pre(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("shape_delete_j_pre", bs, errs); }
{ const bs = mk("plain", B_shape_delete_j_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_shape_delete_j_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("shape_delete_j_post", bs, errs); }
{ const bs = mk("plain", B_shape_add_i_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_shape_add_i_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("shape_add_i_post", bs, errs); }
{ const bs = mk("plain", B_store_other_order_pre); let errs = 0; for (step = 0; step < 40; step++) { try { k_store_other_order_pre(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("store_other_order_pre", bs, errs); }
{ const bs = mk("plain", B_store_other_order_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_store_other_order_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("store_other_order_post", bs, errs); }
{ const bs = mk("plain", B_store_other_next_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_store_other_next_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("store_other_next_post", bs, errs); }
{ const bs = mk("plain", B_callee_store_pre); let errs = 0; for (step = 0; step < 40; step++) { try { k_callee_store_pre(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("callee_store_pre", bs, errs); }
{ const bs = mk("plain", B_callee_store_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_callee_store_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("callee_store_post", bs, errs); }
{ const bs = mk("plain", B_callee_reshape_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_callee_reshape_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("callee_reshape_post", bs, errs); }
{ const bs = mk("plain", B_hole_delete_pre); let errs = 0; for (step = 0; step < 40; step++) { try { k_hole_delete_pre(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("hole_delete_pre", bs, errs); }
{ const bs = mk("plain", B_hole_delete_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_hole_delete_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("hole_delete_post", bs, errs); }
{ const bs = mk("holes", B_holes_start_pre); let errs = 0; for (step = 0; step < 40; step++) { try { k_holes_start_pre(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("holes_start_pre", bs, errs); }
{ const bs = mk("plain", B_len_push_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_len_push_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("len_push_post", bs, errs); }
{ const bs = mk("plain", B_len_push_live_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_len_push_live_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("len_push_live_post", bs, errs); }
{ const bs = mk("plain", B_len_pop_pre); let errs = 0; for (step = 0; step < 40; step++) { try { k_len_pop_pre(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("len_pop_pre", bs, errs); }
{ const bs = mk("plain", B_len_pop_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_len_pop_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("len_pop_post", bs, errs); }
{ const bs = mk("plain", B_len_shrink_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_len_shrink_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("len_shrink_post", bs, errs); }
{ const bs = mk("plain", B_len_grow_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_len_grow_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("len_grow_post", bs, errs); }
{ const bs = mk("plain", B_proto_set_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_proto_set_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("proto_set_post", bs, errs); }
{ const bs = mk("plain", B_proto_set_i_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_proto_set_i_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("proto_set_i_post", bs, errs); }
{ const bs = mk("mixed", B_mixed_start_pre); let errs = 0; for (step = 0; step < 40; step++) { try { k_mixed_start_pre(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("mixed_start_pre", bs, errs); }
{ const bs = mk("extra", B_extra_start_pre); let errs = 0; for (step = 0; step < 40; step++) { try { k_extra_start_pre(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("extra_start_pre", bs, errs); }
{ const bs = mk("plain", B_define_accessor_i_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_define_accessor_i_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("define_accessor_i_post", bs, errs); }
{ const bs = mk("plain", B_define_accessor_j_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_define_accessor_j_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("define_accessor_j_post", bs, errs); }
{ const bs = mk("plain", B_freeze_i_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_freeze_i_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("freeze_i_post", bs, errs); }
{ const bs = mk("plain", B_readonly_j_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_readonly_j_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("readonly_j_post", bs, errs); }
{ const bs = mk("plain", B_store_nonnumber_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_store_nonnumber_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("store_nonnumber_post", bs, errs); }
{ const bs = mk("plain", B_accessor_via_i_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_accessor_via_i_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("accessor_via_i_post", bs, errs); }
{ const bs = mk("plain", B_accessor_via_j_pre); let errs = 0; for (step = 0; step < 40; step++) { try { k_accessor_via_j_pre(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("accessor_via_j_pre", bs, errs); }
{ const bs = mk("plain", B_accessor_via_j_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_accessor_via_j_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("accessor_via_j_post", bs, errs); }
{ const bs = mk("plain", B_freeze_via_i_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_freeze_via_i_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("freeze_via_i_post", bs, errs); }
{ const bs = mk("plain", B_readonly_via_i_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_readonly_via_i_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("readonly_via_i_post", bs, errs); }
{ const bs = mk("plain", B_readonly_via_j_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_readonly_via_j_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("readonly_via_j_post", bs, errs); }
{ const bs = mk("plain", B_delete_via_i_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_delete_via_i_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("delete_via_i_post", bs, errs); }
{ const bs = mk("plain", B_reshape_via_i_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_reshape_via_i_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("reshape_via_i_post", bs, errs); }
{ const bs = mk("plain", B_reshape_via_j_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_reshape_via_j_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("reshape_via_j_post", bs, errs); }
{ const bs = mk("plain", B_add_via_i_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_add_via_i_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("add_via_i_post", bs, errs); }
{ const bs = mk("arraylike", B_array_arraylike_pre); let errs = 0; for (step = 0; step < 40; step++) { try { k_array_arraylike_pre(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("array_arraylike_pre", bs, errs); }
{ const bs = mk("accessor", B_array_accessor_pre); let errs = 0; for (step = 0; step < 40; step++) { try { k_array_accessor_pre(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("array_accessor_pre", bs, errs); }
{ const bs = mk("subclass", B_array_subclass_pre); let errs = 0; for (step = 0; step < 40; step++) { try { k_array_subclass_pre(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("array_subclass_pre", bs, errs); }
{ const bs = mk("plain", B_extra_mid_static_post); let errs = 0; for (step = 0; step < 40; step++) { try { k_extra_mid_static_post(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("extra_mid_static_post", bs, errs); }
{ const bs = mk("plain", B_accessor); let errs = 0; for (step = 0; step < 40; step++) { try { k_accessor(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("accessor", bs, errs); }
{ const bs = mk("holes", B_hole_skip); let errs = 0; for (step = 0; step < 40; step++) { try { k_hole_skip(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("hole_skip", bs, errs); }
{ const bs = mk("plain", B_gc); let errs = 0; for (step = 0; step < 40; step++) { try { k_gc(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("gc", bs, errs); }
{ const bs = mk("mixed", B_gc_mixed); let errs = 0; for (step = 0; step < 40; step++) { try { k_gc_mixed(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("gc_mixed", bs, errs); }
{ const bs = mk("plain", B_gc_grow); let errs = 0; for (step = 0; step < 40; step++) { try { k_gc_grow(bs as any, bs as any, 0.01, bs.length + 0); } catch (e) { errs++; } } report("gc_grow", bs, errs); }
{ const bs = mk("plain", B_oob_bound_skip); const nb = [mk("plain", B_oob_bound_skip), mk("plain", B_oob_bound_skip), mk("plain", B_oob_bound_skip), mk("plain", B_oob_bound_skip)]; let errs = 0; for (step = 0; step < 40; step++) { try { k_oob_bound_skip(bs as any, bs as any, 0.01, bs.length + 40); } catch (e) { errs++; } } report("oob_bound_skip", bs, errs); }
{ const bs = mk("plain", B_oob_bound_throw); const nb = [mk("plain", B_oob_bound_throw), mk("plain", B_oob_bound_throw), mk("plain", B_oob_bound_throw), mk("plain", B_oob_bound_throw)]; let errs = 0; for (step = 0; step < 40; step++) { try { k_oob_bound_throw(bs as any, bs as any, 0.01, bs.length + 40); } catch (e) { errs++; } } report("oob_bound_throw", bs, errs); }
{ const bs = mk("plain", B_oob_far); const nb = [mk("plain", B_oob_far), mk("plain", B_oob_far), mk("plain", B_oob_far), mk("plain", B_oob_far)]; for (step = 0; step < 2; step++) k_oob_far(bs as any, bs as any, 0.01, bs.length + 60000000); console.log("oob_far", sink.toFixed(6), sum(bs), nb.length); }
runBig31(-1);
runBig31(20);
runBig33(-1);
runBig33(20);
console.log("sink", sink > 0);
