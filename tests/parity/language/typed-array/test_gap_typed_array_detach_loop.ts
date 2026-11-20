// A fresh local typed array detached by a callee in the middle of a loop
// (checked tier, region tier, literal and computed lengths). The element
// read precedes the detaching call in the body, so on the next iteration it
// runs after the call. Node: length 0 and `undefined` from then on.
function size(): number { return 10; }
function det8(t: Float64Array): void { (t.buffer as any).transfer(); }
function detU(t: Uint32Array): void { (t.buffer as any).transfer(); }
function detI(t: Int32Array): ArrayBuffer { return (t.buffer as any).transfer(); }

function literal(k: number): number {
  const t = new Float64Array(10);
  for (let i = 0; i < 10; i++) t[i] = i + 1;
  let s = 0;
  for (let i = 0; i < 10; i++) {
    const v = t[i];
    s += v === undefined ? 1000 : v;
    if (i === k) det8(t);
  }
  return s + t.length;
}

function computed(k: number): number {
  const n = size();
  const t = new Uint32Array(n * 1);
  for (let i = 0; i < n; i++) t[i] = i + 1;
  let s = 0;
  for (let r = 0; r < 3; r++) {
    for (let i = 0; i < n; i++) {
      const v = t[i];
      s += v === undefined ? 1000 : v;
    }
    s += t.length;
    if (r === k) detU(t);
  }
  return s;
}

function region(k: number): number {
  const n = size();
  const a = new Float64Array(n * 2);
  for (let i = 0; i < n * 2; i++) a[i] = i;
  let s = 0;
  for (let r = 0; r < 4; r++) {
    for (let i = 0; i < n; i++) {
      const x = a[2 * i] + a[2 * i + 1];
      s += x === x ? x : 1000;
    }
    if (r === k) det8(a);
  }
  return s;
}

function before(k: number): number {
  const t = new Int32Array(16);
  for (let i = 0; i < 16; i++) t[i] = i * 3;
  let s = 0;
  for (let i = 0; i < 16; i++) {
    if (i === k) detI(t);
    const v = t[i];
    s += v === undefined ? 1000 : v;
  }
  return s + t.length + (t[2] === undefined ? 0.5 : 0);
}

function conditionLength(k: number): number {
  const n = size();
  const a = new Uint32Array(n + 6);
  for (let i = 0; i < n + 6; i++) a[i] = i;
  let c = 0;
  for (let i = 0; i < a.length; i++) {
    c += a[i];
    if (i === k) detU(a);
  }
  return c + a.length;
}

// A computed key reaches the prototype getters: `t["buffer"]` observes the
// buffer exactly as `t.buffer` does.
function pick(): string { return "buffer"; }
function stringKey(k: number): number {
  const n = size();
  const t = new Float64Array(n * 1);
  for (let i = 0; i < n; i++) t[i] = i + 1;
  const key = pick();
  let s = 0;
  for (let i = 0; i < n; i++) {
    const v = t[i];
    s += v === undefined ? 1000 : v;
    if (i === k) ((t as any)[key] as any).transfer();
  }
  return s + t.length;
}
console.log("string key:", stringKey(-1), stringKey(2), stringKey(6));

// Exposed from a closure: the statement that detaches never names the array.
function viaClosure(k: number): number {
  const n = size();
  const t = new Float64Array(n * 1);
  for (let i = 0; i < n; i++) t[i] = i + 1;
  const kill = () => { (t.buffer as any).transfer(); };
  let s = 0;
  for (let i = 0; i < n; i++) {
    const v = t[i];
    s += v === undefined ? 1000 : v;
    if (i === k) kill();
  }
  return s + t.length;
}
console.log("closure:", viaClosure(-1), viaClosure(3));

// A module-level array exposed inside another function.
const shared = new Int32Array(12);
function killShared(): void { (shared.buffer as any).transfer(); }
for (let i = 0; i < 12; i++) shared[i] = i;
let sh = 0;
for (let i = 0; i < 12; i++) {
  const v = shared[i];
  sh += v === undefined ? 100 : v;
  if (i === 4) killShared();
}
console.log("module array:", sh, shared.length, shared[0]);

for (let k = -1; k < 12; k += 4) {
  console.log(k, literal(k), computed(k % 4), region(k % 5), before(k), conditionLength(k));
}
let acc = 0;
for (let j = 0; j < 3000; j++) {
  acc += literal(j % 12) + computed(j % 4) + region(j % 5) + before(j % 17) + conditionLength(j % 18);
}
console.log("total", acc);
