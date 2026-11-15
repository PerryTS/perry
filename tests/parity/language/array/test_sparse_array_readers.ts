// A sparse Array keeps a short store under a long `length`: `a[N] = x` far
// past the end, `a.length = N`, `new Array(N)`. Its far elements are ordinary
// own properties, everything between is a hole. Every method reads it by
// HasProperty / Get (or by Get alone, where the method does): a copy holds
// the present elements and holes or `undefined`, never a word from behind
// the store, and a mutator moves the far elements with the rest. Negative
// controls: the same methods on a dense Array of 40 elements.
function sig(a: any): string {
  if (!Array.isArray(a)) return typeof a + ":" + String(a).slice(0, 60);
  let nums = 0, sum = 0, und = 0, other = 0;
  const len = a.length;
  for (let i = 0; i < len; i++) {
    const v = a[i];
    if (v === undefined) und++;
    else if (typeof v === "number") { nums++; sum += v; }
    else other++;
  }
  return "len=" + len + " nums=" + nums + " sum=" + sum + " undefined=" + und + " other=" + other;
}

const N = 1100000;
const MK: [string, () => any[]][] = [
  ["far store", () => { const a: any[] = []; a[N] = 1; return a; }],
  ["length set", () => { const a: any[] = [1, 2, 3]; a.length = N; return a; }],
  ["new Array(N)", () => { const a: any[] = new Array(N); a[1] = 4; return a; }],
  ["two far stores", () => { const a: any[] = [1, 2, 3]; a[N >> 1] = 5; a[N - 1] = 9; return a; }],
  ["numbers", () => { const a: number[] = [1.5, 2.5]; a[N] = 7.5; return a; }],
  ["strings", () => { const a: any[] = ["x", "y"]; a[N] = "z"; return a; }],
  ["dense control", () => { const a: any[] = []; for (let i = 0; i < 40; i++) a.push(i); return a; }],
];

const T: [string, (mk: () => any[]) => any][] = [
  ["base", (mk) => sig(mk())],
  ["slice", (mk) => sig(mk().slice()) + " | " + sig(mk().slice(5, -1)) + " | " + sig(mk().slice(-10))],
  ["concat", (mk) => sig(mk().concat()) + " | " + sig(mk().concat([1, 2], 3)) + " | " + sig([1].concat(mk()))],
  ["concat self", (mk) => { const a = mk(); return sig(a.concat(a)); }],
  ["splice(0,2)", (mk) => { const a = mk(); const r = a.splice(0, 2); return sig(r) + " | " + sig(a); }],
  ["splice(10,len-1000)", (mk) => { const a = mk(); const r = a.splice(10, a.length - 20); return sig(r) + " | " + sig(a); }],
  ["splice(1,0,4,5)", (mk) => { const a = mk(); const r = a.splice(1, 0, 4, 5); return sig(r) + " | " + sig(a); }],
  ["splice(-5)", (mk) => { const a = mk(); const r = a.splice(-5); return sig(r) + " | " + sig(a); }],
  ["splice(2)", (mk) => { const a = mk(); const r = a.splice(2); return sig(r) + " | " + sig(a); }],
  ["spread", (mk) => sig([...mk()]) + " | " + sig([0, ...mk(), 1])],
  ["Array.from", (mk) => sig(Array.from(mk())) + " | " + sig(Array.from(mk(), (x: any) => x))],
  ["map filter", (mk) => sig(mk().map((x: any) => x)) + " | " + sig(mk().filter((x: any) => true))],
  ["flat flatMap", (mk) => sig(mk().flat()) + " | " + sig(mk().flatMap((x: any) => [x, x])) + " | " + sig([[1], mk()].flat().slice(0, 100))],
  ["flatMap sparse result", (mk) => { const s = mk(); return sig([1].flatMap(() => s)); }],
  ["reverse", (mk) => { const a = mk().reverse(); return sig(a) + " first=" + String(a[0]); }],
  ["sort", (mk) => { const a = mk().sort(); return sig(a) + " first=" + String(a[0]); }],
  ["toReversed", (mk) => { const a = (mk() as any).toReversed(); return sig(a) + " first=" + String(a[0]) + " last=" + String(a[a.length - 1]); }],
  ["toSorted", (mk) => { const a = (mk() as any).toSorted(); return sig(a) + " first=" + String(a[0]); }],
  ["toSorted compare", (mk) => { const a = (mk() as any).toSorted((x: any, y: any) => (x < y ? 1 : x > y ? -1 : 0)); return sig(a) + " first=" + String(a[0]); }],
  ["toSpliced", (mk) => sig((mk() as any).toSpliced(0, 1)) + " | " + sig((mk() as any).toSpliced(1, 0, 7, 8)) + " | " + sig((mk() as any).toSpliced(2))],
  ["with", (mk) => { const a = mk(); return sig((a as any).with(0, 4)) + " | " + sig((a as any).with(a.length - 5, 4)) + " | " + sig((a as any).with(-1, 4)); }],
  ["with out of range", (mk) => { const a = mk(); try { (a as any).with(a.length, 1); return "no throw"; } catch (e: any) { return e.constructor.name; } }],
  ["fill copyWithin", (mk) => sig(mk().fill(2, 0, 10)) + " | " + sig(mk().copyWithin(0, 2, 20)) + " | " + sig(mk().copyWithin(5, -20))],
  ["indexOf", (mk) => { const a = mk(); return [a.indexOf(9), a.indexOf(1), a.indexOf(7.5), a.indexOf("z"), a.indexOf(undefined), a.indexOf(12345.5), a.indexOf(1, 5)].join("/"); }],
  ["lastIndexOf", (mk) => { const a = mk(); return [a.lastIndexOf(9), a.lastIndexOf(1), a.lastIndexOf(7.5), a.lastIndexOf("z"), a.lastIndexOf(undefined), a.lastIndexOf(9, -2)].join("/"); }],
  ["includes", (mk) => { const a = mk(); return [a.includes(9), a.includes(1), a.includes(7.5), a.includes("z"), a.includes(undefined), a.includes(12345.5), a.includes(NaN), a.includes(7), a.includes(1, 5)].join("/"); }],
  ["find", (mk) => { const a = mk(); return [a.find((x: any) => x === 9), a.findIndex((x: any) => x === 9), (a as any).findLast((x: any) => x !== undefined), (a as any).findLastIndex((x: any) => x !== undefined)].join("/"); }],
  ["some every forEach reduce", (mk) => { const a = mk(); let n = 0; a.forEach(() => { n++; }); return [a.some((x: any) => x === 9), a.every((x: any) => x !== 12345), n, a.reduce((p: number) => p + 1, 0), a.reduceRight((p: number) => p + 1, 0)].join("/"); }],
  ["join", (mk) => { const a = mk(); return a.join("").length + "/" + String(a).length; }],
  ["at", (mk) => { const a = mk(); return [a.at(-1), a.at(0), a.at(1), a.at(-2), a.at(1000), a.at(a.length >> 1), a[a.length - 1], a[999], a[100000]].map(String).join("/"); }],
  ["shift unshift", (mk) => { const a = mk(); const x = a.shift(); const n = a.unshift(8, 9); return String(x) + " " + n + " " + sig(a); }],
  ["pop push", (mk) => { const a = mk(); const x = a.pop(); const n = a.push(4, 5); const y = a.pop(); return String(x) + " " + n + " " + String(y) + " " + sig(a); }],
  ["push then read", (mk) => { const a = mk(); const far = a.length - 1; a.push(4); return String(a[far]) + "/" + String(a[far + 1]) + "/" + String(a[far - 1]) + " " + sig(a); }],
  ["store near the end", (mk) => { const a = mk(); const len = a.length; a[len - 2] = 7; a[len + 5] = 8; return sig(a) + " " + String(a[len - 1]) + "/" + String(a[len - 2]) + "/" + String(a[len + 5]); }],
  ["store low", (mk) => { const a = mk(); a[20] = 7; a[5000] = 8; return sig(a) + " " + String(a[20]) + "/" + String(a[5000]) + "/" + String(a[a.length - 1]); }],
  ["length shrink", (mk) => { const a = mk(); a.length = 5; return sig(a) + " | " + sig(a.concat()) + " | " + sig(a.slice()) + " | " + sig([...a]); }],
  ["length shrink then grow", (mk) => { const a = mk(); a.length = 2; a.length = 300; a.push(6); return sig(a) + " | " + sig(a.concat()); }],
  ["length grow", (mk) => { const a = mk(); a.length = a.length + 100; return sig(a) + " | " + sig((a as any).toReversed().slice(0, 200)); }],
  ["delete in", (mk) => { const a = mk(); const far = a.length - 1; const r = [far in a, 0 in a, 17 in a, a.hasOwnProperty(far)].join("/"); delete a[far]; delete a[0]; return r + " " + [far in a, 0 in a].join("/") + " " + sig(a); }],
  ["for-of entries", (mk) => { let n = 0, c = 0; for (const v of mk()) { c++; if (v !== undefined) n++; } let e = 0; for (const [i, v] of mk().entries()) { if (v !== undefined) e += i; } return n + "/" + c + "/" + e; }],
  ["destructure", (mk) => { const [a, b, c, d] = mk(); const [h, ...rest] = mk(); return [a, b, c, d, h].map(String).join(",") + " " + sig(rest); }],
  ["json", (mk) => { const s = JSON.stringify(mk()); return s.length + ":" + s.slice(0, 24) + ":" + s.slice(-16); }],
  ["typed array from", (mk) => { const t = new Float64Array(mk().map((x: any) => (typeof x === "number" ? x : 0))); let n = 0; for (let i = 0; i < t.length; i++) if (t[i] !== 0) n++; return t.length + "/" + n; }],
  ["call through prototype", (mk) => { const a = mk(); return [Array.prototype.indexOf.call(a, a[a.length - 1]), Array.prototype.includes.call(a, 12345.5), Array.prototype.lastIndexOf.call(a, 12345.5)].join("/") + " " + sig(Array.prototype.slice.call(a, -3)); }],
];

for (const [name, mk] of MK) {
  for (const [test, run] of T) {
    let r: string;
    try {
      r = String(run(mk));
    } catch (e: any) {
      r = "throw " + (e && e.constructor && e.constructor.name);
    }
    console.log(name + ": " + test + " -> " + r);
  }
}
