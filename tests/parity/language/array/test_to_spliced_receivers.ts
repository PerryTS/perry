// toSpliced / toSorted / toReversed / with on receivers that are no plain
// Array: an Array-subclass instance (parameter, field, module binding, local,
// `any`), a species-overriding subclass, an Array with a named property or an
// own `toSpliced`, array-likes and a string through `.call`, a Proxy. Each
// returns a plain Array (no species); toSpliced converts start, then
// skipCount, before it reads an element. Control: plain Arrays.
class P extends Array<number> {}
class Q extends Array<number> { static get [Symbol.species]() { return Array; } extra() { return "q"; } }
function show(label: string, f: () => any) {
  let s: string;
  try { const r = f(); s = (Array.isArray(r) ? "A" : typeof r) + ":" + (r && r.constructor && r.constructor.name) + ":" + JSON.stringify(r) + ":" + (r instanceof P); }
  catch (e: any) { s = "THROW " + e.constructor.name; }
  console.log(label, s);
}
function mkP(): P { const p = new P(); p.push(5, 1, 4, 2); return p; }
function mkQ(): Q { const q = new Q(); q.push(3, 9, 7); return q; }
function viaParam(p: P) { return [p.toSpliced(0, 1), p.toSorted(), p.toReversed(), p.with(1, 8), p.toSpliced(1), p.toSpliced(), p.toSpliced(-2, 1, 10, 11)]; }
class Holder { p: P = mkP(); run() { return [this.p.toSpliced(0, 1), this.p.toSorted((a, b) => b - a), this.p.toReversed(), this.p.with(-1, 0)]; } }
const modP = mkP();
for (const [i, r] of viaParam(mkP()).entries()) show("param" + i, () => r);
for (const [i, r] of new Holder().run().entries()) show("field" + i, () => r);
show("module", () => modP.toSpliced(0, 2, 99));
show("local", () => { const p = mkP(); return p.toSpliced(1, 2); });
show("q", () => mkQ().toSpliced(0, 1));
show("q sorted", () => mkQ().toSorted());
show("any", () => { const p: any = mkP(); return [p.toSpliced(0, 1), p.toSorted(), p.toReversed(), p.with(0, 1)]; });
show("named prop", () => { const a: any = [3, 1, 2]; a.toSpliced = Array.prototype.toSpliced; return a.toSpliced(0, 1); });
show("named prop2", () => { const a: any = [3, 1, 2]; a.foo = 1; return [a.toSpliced(1, 1), a.toSorted(), a.toReversed(), a.with(0, 7)]; });
show("call arraylike", () => Array.prototype.toSpliced.call({ length: 3, 0: "a", 2: "c" }, 1, 1, "x"));
show("call string", () => (Array.prototype as any).toSpliced.call("abc", 1, 1));
show("holes", () => { const p = new P(); p.length = 3; p[1] = 7; return p.toSpliced(0, 0); });
show("getter order", () => { const log: string[] = []; const o: any = { get length() { log.push("len"); return 2; }, get 0() { log.push("g0"); return "a"; }, get 1() { log.push("g1"); return "b"; } }; const r = Array.prototype.toSpliced.call(o, { valueOf() { log.push("start"); return 1; } } as any, { valueOf() { log.push("skip"); return 0; } } as any, "x"); return [r, log]; });
show("with oob", () => mkP().with(10, 1));
show("toSorted bad cmp", () => (mkP() as any).toSorted(5));
show("proxy", () => { const p = new Proxy(mkP(), {}); return [p.toSpliced(0, 1), p.toSorted()]; });
show("plain control", () => [[3, 1, 2].toSpliced(0, 1), [3, 1, 2].toSorted(), [3, 1, 2].toReversed(), [3, 1, 2].with(1, 5)]);
