// keys qfa q1e9 qacc: FNV-1a hashes 70d03c1952fcee7d, 164292068c1a6e7d, cb3f9808149f6e7d share their low 14 bits, so they share their first bucket in
// every key index (shape KeyIndex, overflow table) of up to 16384 buckets. A is placed before B; C is never own.

const A = "qfa", B = "q1e9", C = "qacc";
const keys = [A, B, C];
function show(tag, o) { const r = []; for (const k of keys) r.push(k + "=" + String(o[k]) + "/" + (k in o)); console.log(tag, r.join(" "), Object.keys(o).length); }
function fill(o, n, p) { for (let i = 0; i < n; i++) o[p + i] = i; return o; }
// static sites on the colliding keys
function sread(o) { return [o.qfa, o.q1e9, o.qacc]; }
function swrite(o, v) { o.qfa = v; o.q1e9 = v + 1; }
function sin(o) { return ["qfa" in o, "q1e9" in o, "qacc" in o]; }
function sdelA(o) { return delete o.qfa; }
function sdelB(o) { return delete o.q1e9; }
function run(tag, make) {
  for (let round = 0; round < 3; round++) {
    const o = make();
    show(tag + " born" + round, o);
    console.log(tag, "sread", sread(o).join(","), "sin", sin(o).join(","));
    swrite(o, 10 + round); show(tag + " swrite", o);
    for (const k of keys) o[k] = k + round;          // keyed write, C is added
    show(tag + " kwrite", o);
    console.log(tag, "sdelA", sdelA(o)); show(tag + " -A", o);
    console.log(tag, "sread", sread(o).join(","), "sin", sin(o).join(","));
    o[B] = "b" + round; o.qfa = "a" + round;           // B after A is gone, A re-added behind B
    show(tag + " readd", o);
    console.log(tag, "kdelB", delete o[B]); show(tag + " -B", o);
    console.log(tag, "sdelB", sdelB(o), "kdelC", delete o[C], "kdelA", delete o[A]); show(tag + " empty", o);
    o[A] = 1; o[B] = 2; console.log(tag, "end", sread(o).join(","), sin(o).join(","), JSON.stringify(Object.keys(o).slice(-3)));
  }
}
run("small", () => ({ [A]: 1, [B]: 2 }));
run("indexed", () => { const o = fill({}, 10, "f"); o[A] = 1; o[B] = 2; return o; });
run("indexed-rev", () => { const o = fill({}, 10, "g"); o[B] = 2; o[A] = 1; return o; });
run("literal", () => ({ f0: 0, f1: 1, f2: 2, f3: 3, f4: 4, f5: 5, f6: 6, f7: 7, f8: 8, qfa: 1, q1e9: 2 }));
run("overflowed", () => { const o = fill({}, 40, "h"); for (let i = 3; i < 40; i += 3) delete o["h" + i]; o[A] = 1; o[B] = 2; return o; });
run("dict", () => { const o = {}; for (let i = 0; i < 200; i++) o["d" + i] = i; for (let i = 0; i < 200; i += 2) delete o["d" + i]; o[A] = 1; o[B] = 2; return o; });
class K { constructor() { this.x = 1; this[A] = 1; this[B] = 2; } }
run("class", () => fill(new K(), 9, "c"));
