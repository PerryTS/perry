// P2 typed signatures: a method whose parameters feed element keys is typed
// when the facts give them F64 (a Number on every call that is no proven
// int32: a counter bounded by a length, a fraction). The typed body converts
// the key exactly at each element access, so it replaces the method's index
// clones. Exact against node with the rule on, off (PERRY_SIG_NUMBER_INDEX=0)
// and in check mode.
// A loop bound the compiler cannot fold keeps each loop as written.
const RUN = Math.random() < 2 ? 1 : 0;

// 1. The shape: small accessors over plain Arrays, keys from a counter whose
// bound is a length.
class Store {
  data: number[] = [];
  flags: number[] = [];
  constructor(n: number) {
    for (let i = 0; i < n; i++) {
      this.data.push(i * 0.5);
      this.flags.push(i & 3);
    }
  }
  get(i: number): number {
    return this.data[i];
  }
  flag(i: number): number {
    return this.flags[i];
  }
  set(i: number, v: number): void {
    this.data[i] = v;
  }
  size(): number {
    return this.data.length;
  }
}
function sweep(s: Store, rounds: number): number {
  let acc = 0;
  for (let r = 0; r < rounds; r++) {
    const n = s.size();
    for (let i = 0; i < n; i++) {
      if (s.flag(i) !== 0) acc += s.get(i);
      s.set(i, s.get(i) + 1);
    }
  }
  return acc;
}
function shape(): void {
  const s = new Store(100 * RUN);
  console.log("sweep", sweep(s, 7), s.get(0), s.get(99), s.size());
}
shape();

// 2. Keys that are Numbers and no element index: a fraction, a negative,
// -0, NaN, the infinities, a key past 2^32. A read sees `undefined` (or the
// property a store of the same key made); a store names a property and
// leaves the length alone.
function reads(s: Store): string {
  return [
    s.get(1.5),
    s.get(-1),
    s.get(-0),
    s.get(NaN),
    s.get(Infinity),
    s.get(-Infinity),
    s.get(4294967296),
    s.get(4294967295),
    s.get(0.1 + 0.2),
    s.get(7),
    s.get(8),
    s.get(9.000000000000002),
  ]
    .map(String)
    .join(",");
}
function odd(): void {
  const s = new Store(8 * RUN);
  console.log("odd read", reads(s));
  s.set(1.5, 15);
  s.set(-1, -10);
  s.set(-0, 100);
  s.set(NaN, 77);
  s.set(Infinity, 88);
  s.set(0.1 + 0.2, 3);
  console.log("odd write", reads(s), s.size(), Object.keys(s.data).join("|"));
  s.set(8, 800);
  s.set(12, 1200);
  console.log("grow", s.size(), s.get(8), String(s.get(9)), s.get(12), s.get(11.999999999999998) === undefined);
}
odd();

// 3. Half of the keys of one loop are fractions: the same body takes both.
function mixed(): void {
  const s = new Store(40 * RUN);
  let hits = 0;
  let misses = 0;
  let acc = 0;
  for (let i = -4; i < 100 * RUN; i++) {
    const v = s.get(i / 2);
    if (v === undefined) misses++;
    else {
      hits++;
      acc += v;
    }
  }
  console.log("mixed", hits, misses, acc);
}
mixed();

// 4. Control: a key that may be `undefined` (an element of an Array with a
// hole) is no Number on every call. The method keeps its guarded clones and
// the output is still exact.
class Column {
  values: number[];
  constructor(n: number) {
    this.values = new Array<number>(n).fill(0);
  }
  read(entity: number): number {
    return this.values[entity];
  }
  write(entity: number, v: number): void {
    this.values[entity] = v;
  }
}
function control(): void {
  const ids: number[] = [];
  for (let i = 0; i < 12 * RUN; i++) ids.push(i);
  (ids as any)[14] = 3;
  const c = new Column(12);
  for (let k = 0; k < ids.length; k++) c.write(ids[k], k * 2);
  let sum = 0;
  let undef = 0;
  for (let k = 0; k < ids.length; k++) {
    const v = c.read(ids[k]);
    if (v === undefined) undef++;
    else sum += v;
  }
  console.log("control", sum, undef, c.values.length, Object.keys(c.values).length, (c.values as any)["undefined"]);
}
control();

// 5. Control: a string reaches the parameter through an untyped caller.
class Loose {
  data: number[] = [];
  constructor(n: number) {
    for (let i = 0; i < n; i++) this.data.push(i * 3);
  }
  get(i: number): number {
    return this.data[i];
  }
  set(i: number, v: number): void {
    this.data[i] = v;
  }
}
function untyped(): void {
  const s: any = new Loose(6 * RUN);
  console.log("untyped", s.get("2"), s.get("length"), String(s.get("x")), s.get(2), s.get(2.5) === undefined);
  s.set("4", 44);
  s.set("name", 5);
  console.log("untyped write", s.get(4), s.data.name, s.data.length);
}
untyped();
