// Scope context objects: captured-and-mutated bindings shared through one GC
// cell per scope activation. Every read and write must see the same binding
// from the defining frame and every closure, across moving collections.
declare function gc(): void;
function collect() {
  if (typeof gc === "function") gc();
}

// Sibling closures sharing several mutated bindings of one scope.
function siblings(seed: number) {
  let a = seed;
  let b = seed * 2;
  let label = "s" + seed;
  const incA = () => { a++; collect(); return a; };
  const incB = () => { b += a; return b; };
  const rename = (s: string) => { label = label + s; return label; };
  const snapshot = () => `${a}/${b}/${label}`;
  a += 10; // the frame writes too
  return { incA, incB, rename, snapshot };
}
const s1 = siblings(1);
const s2 = siblings(5);
console.log(s1.incA(), s1.incB(), s1.rename("x"), s1.snapshot());
console.log(s2.incA(), s2.incA(), s2.incB(), s2.snapshot(), s1.snapshot());

// Different closure sets: `x` is shared by f and g, `y` only by g, `z` only by
// f. The bindings land in different scope objects; all stay consistent.
function splitGroups() {
  let x = 0;
  let y = 100;
  let z = 1000;
  const f = () => { x++; z--; return x + z; };
  const g = () => { x += 2; y++; return x + y; };
  x = 5;
  collect();
  return [f(), g(), f(), g(), x, y, z].join(",");
}
console.log(splitGroups());

// Nested scopes: an inner block scope and the function scope both captured by
// one closure, and a deeper closure forwarding an outer binding.
function nested(n: number) {
  let outer = n;
  const fns: Array<() => number> = [];
  {
    let inner = n * 10;
    fns.push(() => { outer++; inner++; return outer + inner; });
    fns.push(() => {
      const deeper = () => { outer *= 2; return outer; };
      return deeper() + inner;
    });
  }
  outer += 1;
  collect();
  return fns.map((f) => f()).concat([outer]).join(" ");
}
console.log(nested(3));

// Hoisted function declarations capturing sibling `var`s and forward `let`s
// (the module-factory shape): every binding is preallocated at entry.
function factory(exportsObj: any) {
  var counter = 0;
  var cache: Record<string, number> = {};
  function get(key: string) {
    counter++;
    if (!(key in cache)) cache[key] = counter * 10 + later;
    return cache[key];
  }
  function reset() {
    cache = {};
    counter = 0;
  }
  let later = 7;
  exportsObj.get = get;
  exportsObj.reset = reset;
  exportsObj.stats = () => `${counter}:${Object.keys(cache).length}`;
  later = 9;
  return exportsObj;
}
const mod = factory({});
console.log(mod.get("a"), mod.get("b"), mod.get("a"), mod.stats());
collect();
mod.reset();
console.log(mod.get("c"), mod.stats());

// Many mutated bindings in one frame (the #8132 wide-frame shape), captured by
// one closure and mutated across allocating calls.
function wide(rounds: number) {
  let v0 = 0, v1 = 1, v2 = 2, v3 = 3, v4 = 4, v5 = 5, v6 = 6, v7 = 7;
  let v8 = 8, v9 = 9, v10 = 10, v11 = 11, v12 = 12, v13 = 13, v14 = 14, v15 = 15;
  let objs: any[] = [];
  const touch = () => {
    v0++; v1++; v2++; v3++; v4++; v5++; v6++; v7++;
    v8++; v9++; v10++; v11++; v12++; v13++; v14++; v15++;
    objs.push({ v0, v15 });
  };
  for (let i = 0; i < rounds; i++) {
    touch();
    if (i % 7 === 0) collect();
    v0 += objs.length;
  }
  return [v0, v1, v7, v8, v15, objs.length, objs[objs.length - 1].v15].join(",");
}
console.log(wide(50));

// Closures escaping into a long-lived structure keep their own scope alive.
const registry: Array<{ next: () => number; name: () => string }> = [];
function register(prefix: string) {
  let n = 0;
  let name = prefix;
  registry.push({
    next: () => ++n,
    name: () => (name = name + n),
  });
}
for (let i = 0; i < 5; i++) register("r" + i);
collect();
for (let round = 0; round < 3; round++) {
  for (const entry of registry) entry.next();
  collect();
}
console.log(registry.map((e) => e.name()).join(" "));

// Self-recursive closure and a closure capturing its own binding.
function recursive() {
  let depth = 0;
  let fib = (k: number): number => {
    depth++;
    return k < 2 ? k : fib(k - 1) + fib(k - 2);
  };
  const r = fib(12);
  fib = (k: number) => -k;
  return `${r} ${depth} ${fib(3)}`;
}
console.log(recursive());

// Compound assignment, update and string append through a scope slot.
function compound() {
  let s = "";
  let n = 1;
  const add = (t: string) => { s += t; n *= 3; n -= 1; return s.length; };
  for (let i = 0; i < 4; i++) add("ab" + i);
  return `${s} ${n} ${s.length}`;
}
console.log(compound());

// Capture by value: every write precedes every capturing closure, so the
// binding needs no cell. The mirror case (a write after the capture) must
// still share one binding.
function byValue(flag: boolean) {
  let base: number;
  if (flag) base = 10;
  else base = 20;
  base += 1;
  const read = () => base * 2;
  let late = 1;
  const readLate = () => late;
  late = 2;
  collect();
  return `${read()} ${readLate()}`;
}
console.log(byValue(true), byValue(false));
