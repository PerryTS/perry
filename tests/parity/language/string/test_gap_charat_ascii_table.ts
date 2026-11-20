// charAt / s[i] on ASCII, SSO, non-ASCII and out-of-range inputs; the
// one-character results are ordinary strings: equal, usable as keys, and
// appendable.
const heap = "The quick brown fox jumps over the lazy dog 0123456789 -._~!*()";
let out = "";
for (let i = 0; i < heap.length; i++) out += heap.charAt(i);
console.log(out === heap, out.length);
const sso = String(12345);
console.log(sso.charAt(0), sso.charAt(4), JSON.stringify(sso.charAt(5)), sso[2]);
const uni = "héllo wörld €";
const parts: string[] = [];
for (let i = 0; i < uni.length; i++) parts[parts.length] = uni.charAt(i);
console.log(parts.join("|"), parts.length);
console.log(JSON.stringify(heap.charAt(-1)), JSON.stringify(heap.charAt(999)), heap.charAt(NaN as any), heap.charAt(1.9), heap.charAt("3" as any), heap.charAt(undefined as any), heap.charAt());
const ctl = "\u0000\u0001\t\n\u007f";
for (let i = 0; i < ctl.length; i++) console.log(ctl.charCodeAt(i), ctl.charAt(i).length, ctl.charAt(i) === ctl[i]);
const counts: Record<string, number> = {};
for (let i = 0; i < heap.length; i++) { const c = heap.charAt(i); counts[c] = (counts[c] || 0) + 1; }
console.log(counts["o"], counts[" "], counts["x"], counts["y"], Object.keys(counts).length);
const m = new Map<string, number>();
for (let i = 0; i < 10; i++) m.set(heap.charAt(i % 3), i);
console.log([...m.entries()].join(";"));
const o: any = { T: 1, h: 2, e: 3 };
console.log(o[heap.charAt(0)], o[heap.charAt(1)], o[heap.charAt(2)], heap.charAt(0) === "T", typeof heap.charAt(0));
const arr: string[] = [];
for (let i = 0; i < 5; i++) arr[arr.length] = heap.charAt(i) + heap.charAt(i).toUpperCase();
console.log(arr.join(","), arr.length);
let s2 = heap.charAt(4); s2 += "uick"; console.log(s2, heap.charAt(4));
