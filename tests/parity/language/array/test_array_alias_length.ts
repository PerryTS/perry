// Named lengths (wpfacts `named_lengths`): `for (let i = 0; i < a.length;
// i++)` with no call in its body keeps `a.length` only when no store of the
// body can change it. A store through another name counts when that name
// may hold the Array `a` holds: the B cases change the length through an
// alias the text of the loop does not show (each printed one step too many,
// or too few, before the fact existed); the K cases store through another
// name into another Array and keep the hoisted loop; the O cases store
// through the walked name itself. Each case bears its Arrays at its own
// literal: an Array token is its birth site.

let k = 1;
const FIX: number[] = [7, 8, 9];

// B1: a conditional alias of a module binding, popped by the binding's name.
const G1: number[] = [1, 2, 3, 4, 5, 6];
function b1(): string {
  const xs = k ? G1 : FIX;
  let s = "";
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] + ",";
    G1.pop();
  }
  return s;
}

// B2: shift.
const G2: number[] = [1, 2, 3, 4, 5, 6];
function b2(): string {
  const xs = k ? G2 : FIX;
  let s = "";
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] + ",";
    G2.shift();
  }
  return s;
}

// B3: splice.
const G3: number[] = [1, 2, 3, 4, 5, 6];
function b3(): string {
  const xs = k ? G3 : FIX;
  let s = "";
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] + ",";
    G3.splice(0, 1);
  }
  return s;
}

// B4: an assignment of `length`.
const G4: number[] = [1, 2, 3, 4, 5, 6];
function b4(): string {
  const xs = k ? G4 : FIX;
  let s = "";
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] + ",";
    G4.length = G4.length - 1;
  }
  return s;
}

// B5: growth through the alias: a kept length would stop too early.
const G5: number[] = [1, 2, 3, 4, 5, 6];
function b5(): string {
  const xs = k ? G5 : FIX;
  let s = "";
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] + ",";
    if (i < 3) G5.push(10 + i);
  }
  return s;
}

// B6: two parameters bound to one Array.
function b6(a: number[], b: number[]): string {
  let s = "";
  for (let i = 0; i < a.length; i++) {
    s += a[i] + ",";
    b.pop();
  }
  return s;
}

// B7: the alias is a call's result.
const G7: number[] = [1, 2, 3, 4, 5, 6];
function pick7(): number[] {
  return k ? G7 : FIX;
}
function b7(): string {
  const xs = pick7();
  let s = "";
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] + ",";
    G7.pop();
  }
  return s;
}

// B8: the alias is a field.
const G8: number[] = [1, 2, 3, 4, 5, 6];
class Box {
  items: number[];
  constructor(a: number[]) {
    this.items = a;
  }
  // read from the field, popped by the binding's name
  walk(): string {
    const xs = this.items;
    let s = "";
    for (let i = 0; i < xs.length; i++) {
      s += xs[i] + ",";
      G8.pop();
    }
    return s;
  }
  // a parameter, popped through the field
  walkPop(a: number[]): string {
    let s = "";
    for (let i = 0; i < a.length; i++) {
      s += a[i] + ",";
      this.items.pop();
    }
    return s;
  }
  // a parameter, grown by an index store through the field
  walkGrow(a: number[]): string {
    let s = "";
    for (let i = 0; i < a.length; i++) {
      s += a[i] + ",";
      if (i < 3) this.items[this.items.length] = 20 + i;
    }
    return s;
  }
}

// B9: a local a closure captures, popped by its own name.
function b9(): string {
  const own = [1, 2, 3, 4, 5, 6];
  const xs = k ? own : FIX;
  const len = () => own.length;
  let s = "";
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] + ",";
    own.pop();
  }
  return s + len();
}

// B10: an index store through the other parameter grows the Array.
function b10(a: number[], b: number[]): string {
  let s = "";
  for (let i = 0; i < a.length; i++) {
    s += a[i] + ",";
    if (i < 3) b[b.length] = 30 + i;
  }
  return s;
}

// B11: the alias is read from an object literal's field.
const G11: number[] = [1, 2, 3, 4, 5, 6];
function b11(): string {
  const o = { v: G11 };
  const xs = o.v;
  let s = "";
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] + ",";
    G11.pop();
  }
  return s;
}

// B12: the alias the text shows (`const xs = G`).
const G12: number[] = [1, 2, 3, 4, 5, 6];
function b12(): string {
  const xs = G12;
  let s = "";
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] + ",";
    G12.pop();
  }
  return s;
}

// B13: a `let` assigned before the loop.
const G13: number[] = [1, 2, 3, 4, 5, 6];
function b13(): string {
  let xs = FIX;
  if (k) xs = G13;
  let s = "";
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] + ",";
    G13.pop();
  }
  return s;
}

// B14: both names are locals.
function b14(): string {
  const own = [1, 2, 3, 4, 5, 6];
  const xs = k ? own : FIX;
  let s = "";
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] + ",";
    own.pop();
  }
  return s;
}

// B15: the alias is an element of an Array of Arrays, grown through it.
function b15(): string {
  const rows = [[1, 2, 3, 4, 5, 6], [1, 2]];
  const xs = rows[0];
  let s = "";
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] + ",";
    if (i < 2) rows[0][rows[0].length] = 40 + i;
  }
  return s;
}

// B16: unshift through the alias.
function b16(): string {
  const own = [1, 2, 3, 4];
  const xs = k ? own : FIX;
  let s = "";
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] + ",";
    if (i < 2) own.unshift(0);
  }
  return s;
}

// B17: a `<=` bound.
function b17(): string {
  const own = [1, 2, 3, 4, 5, 6];
  const xs = k ? own : FIX;
  let s = "";
  for (let i = 0; i + 1 <= xs.length; i++) {
    s += xs[i] + ",";
    own.pop();
  }
  return s;
}

// B18: the alias is made by an assignment whose value is a sequence.
function b18(): string {
  const own = [1, 2, 3, 4, 5, 6];
  let ys: number[] = FIX;
  ys = (k, own);
  let s = "";
  for (let i = 0; i < own.length; i++) {
    s += own[i] + ",";
    ys.pop();
  }
  return s;
}

// B19: the store's receiver is an assignment that hands the Array on.
function b19(): string {
  const own = [1, 2, 3];
  let ys: number[] = FIX;
  let s = "";
  for (let i = 0; i < own.length; i++) {
    s += own[i] + ",";
    if (i < 2) (ys = own)[own.length] = 50 + i;
  }
  return s + ys.length;
}

// B20: an in-bounds store through the other name at the loop's counter
// changes no length, whichever Array it is; one past it does.
function b20(a: number[], b: number[]): string {
  let s = "";
  for (let i = 0; i < a.length; i++) {
    b[i] = a[i] * 2;
    s += a[i] + ",";
  }
  return s;
}
function b21(a: number[], b: number[]): string {
  let s = "";
  for (let i = 0; i < a.length; i++) {
    if (i < 3) b[i + 6] = i;
    s += a[i] + ",";
  }
  return s;
}

// B22: a nested loop over the same Array through two names.
function b22(a: number[], b: number[]): string {
  let s = "";
  for (let i = 0; i < a.length; i++) {
    for (let j = 0; j < b.length; j++) {
      if (j === 0 && a.length > 2) a.pop();
      s += b[j];
    }
    s += ",";
  }
  return s;
}

// B23: the loop is in a closure over the walked local; the other name is
// the closure's parameter.
function b23(): string {
  const own = [1, 2, 3, 4, 5, 6];
  const run = (other: number[]): string => {
    let s = "";
    for (let i = 0; i < own.length; i++) {
      s += own[i] + ",";
      other.pop();
    }
    return s;
  };
  return run(own);
}

// B24: a module-scope loop.
const G24: number[] = [1, 2, 3, 4, 5, 6];
const A24 = k ? G24 : FIX;
let s24 = "";
for (let i = 0; i < A24.length; i++) {
  s24 += A24[i] + ",";
  G24.pop();
}

// T: the loop's other tiers (float elements and a long walk, a store
// through the walked name beside growth through the other, a nested walk,
// object elements, string elements).
function t1(): string {
  const own: number[] = [];
  for (let i = 0; i < 1000; i++) own.push(i * 0.5);
  const xs = k ? own : [1.5];
  let acc = 0;
  let n = 0;
  for (let j = 0; j < xs.length; j++) {
    acc += xs[j];
    n++;
    own.pop();
  }
  return acc + ":" + n;
}
function t2(a: number[], b: number[]): string {
  let n = 0;
  for (let j = 0; j < a.length; j++) {
    a[j] = a[j] * 2;
    n++;
    if (j < 5) b.push(j);
  }
  return n + ":" + a.length;
}
function t3(a: number[], b: number[]): string {
  let s = 0;
  let n = 0;
  for (let i = 0; i < a.length; i++) {
    for (let j = 0; j < b.length; j++) {
      s += b[j];
      n++;
    }
    a.pop();
  }
  return s + ":" + n;
}
function t4(s: string[], t: string[]): string {
  let out = "";
  for (let j = 0; j < s.length; j++) {
    out += s[j];
    t.shift();
  }
  return out;
}
interface Cell {
  v: number;
}
function t5(): string {
  const own: Cell[] = [{ v: 1 }, { v: 2 }, { v: 3 }, { v: 4 }];
  const xs = k ? own : [];
  let n = 0;
  for (let j = 0; j < xs.length; j++) {
    xs[j].v = xs[j].v + 1;
    n++;
    if (j < 2) own.push({ v: 9 });
  }
  let s = "";
  for (let j = 0; j < own.length; j++) s += own[j].v + ",";
  return n + ":" + s;
}
function t6(): string {
  const own: Cell[] = [{ v: 1 }, { v: 2 }, { v: 3 }, { v: 4 }, { v: 5 }, { v: 6 }];
  const other: Cell[] = [{ v: 7 }];
  const xs = k ? own : other;
  let acc = 0;
  let n = 0;
  for (let j = 0; j < xs.length; j++) {
    acc += xs[j].v;
    n++;
    own.pop();
  }
  return acc + ":" + n;
}

// K: stores through another name into another Array: the loop is the
// hoisted loop, by the fact.
function k1(): string {
  const src = [1, 2, 3, 4, 5, 6];
  const out: number[] = [];
  for (let i = 0; i < src.length; i++) {
    out.push(src[i] * 2);
  }
  let s = "";
  for (let i = 0; i < out.length; i++) s += out[i] + ",";
  return s;
}
function k2(): string {
  const src = [1, 2, 3, 4, 5, 6];
  const hist = [0, 0, 0];
  for (let i = 0; i < src.length; i++) {
    hist[src[i] % 3] += 1;
  }
  return hist.join(",");
}
function k3(a: number[], out: number[]): string {
  for (let i = 0; i < a.length; i++) {
    out.pop();
    out.push(a[i]);
    out.push(a[i] + 1);
  }
  return out.join(",");
}
class Acc {
  total: number[] = [0, 0];
  add(a: number[]): string {
    for (let i = 0; i < a.length; i++) {
      this.total[i & 1] += a[i];
      this.total[this.total.length] = i;
    }
    return this.total.join(",");
  }
}

// O: stores through the walked name itself.
function o1(): string {
  const xs = [1, 2, 3, 4, 5, 6];
  for (let i = 0; i < xs.length; i++) xs[i] = xs[i] * 3;
  return xs.join(",");
}
function o2(): string {
  const xs = [1, 2, 3, 4, 5, 6];
  let s = "";
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] + ",";
    xs.pop();
  }
  return s;
}
function o3(): string {
  const xs = [1, 2, 3];
  let s = "";
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] + ",";
    if (i < 3) xs.push(60 + i);
  }
  return s;
}
function o4(xs: number[] | undefined): string {
  let s = "";
  if (xs) {
    for (let i = 0; i < xs.length; i++) {
      xs[i] = xs[i] + 1;
      s += xs[i] + ",";
    }
  }
  return s;
}

console.log("b1", b1());
console.log("b2", b2());
console.log("b3", b3());
console.log("b4", b4());
console.log("b5", b5());
const s6 = [1, 2, 3, 4, 5, 6];
console.log("b6", b6(s6, s6));
console.log("b7", b7());
const bx = new Box(G8);
console.log("b8a", bx.walk());
const g8b = [1, 2, 3, 4, 5, 6];
console.log("b8b", new Box(g8b).walkPop(g8b));
const g8c = [1, 2, 3, 4, 5, 6];
console.log("b8c", new Box(g8c).walkGrow(g8c));
console.log("b9", b9());
const s10 = [1, 2, 3, 4, 5, 6];
console.log("b10", b10(s10, s10));
console.log("b11", b11());
console.log("b12", b12());
console.log("b13", b13());
console.log("b14", b14());
console.log("b15", b15());
console.log("b16", b16());
console.log("b17", b17());
console.log("b18", b18());
console.log("b19", b19());
const s20 = [1, 2, 3, 4, 5, 6];
console.log("b20", b20(s20, s20), s20.join(","));
const s21 = [1, 2, 3, 4, 5, 6];
console.log("b21", b21(s21, s21), s21.length);
const s22 = [1, 2, 3, 4, 5, 6];
console.log("b22", b22(s22, s22));
console.log("b23", b23());
console.log("b24", s24);
console.log("t1", t1());
const s2: number[] = [];
for (let i = 0; i < 10; i++) s2.push(i + 0.5);
console.log("t2", t2(s2, s2));
const s3 = [1, 2, 3, 4, 5, 6];
console.log("t3", t3(s3, s3));
const s4 = ["a", "b", "c", "d", "e", "f"];
console.log("t4", t4(s4, s4));
console.log("t5", t5());
console.log("k1", k1());
console.log("k2", k2());
console.log("k3", k3([1, 2, 3], [9]));
console.log("k4", new Acc().add([5, 6, 7]));
console.log("o1", o1());
console.log("o2", o2());
console.log("o3", o3());
console.log("o4", o4([1, 2, 3]), o4(undefined));

// The same loops with the other Array: the body's store leaves the walked
// Array alone.
k = 0;
const H1: number[] = [1, 2, 3, 4, 5, 6];
function n1(): string {
  const xs = k ? H1 : FIX;
  let s = "";
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] + ",";
    H1.pop();
  }
  return s + H1.length;
}
console.log("n1", n1());
console.log("n6", b6([1, 2, 3], [4, 5, 6, 7]));
console.log("n10", b10([1, 2, 3], [4, 5]));
console.log("n14", b14());
console.log("n20", b20([1, 2, 3], [0, 0, 0, 0]));
console.log("n22", b22([1, 2, 3, 4], [5, 6]));
k = 1;
console.log("t6", t6());
