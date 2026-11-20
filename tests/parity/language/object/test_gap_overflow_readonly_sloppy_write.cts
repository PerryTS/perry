// Sloppy-mode script: a write to a read-only key of an object with an
// overflow is silently ignored; a strict function throws TypeError.
function mk(n: number): any {
  const o: any = { a: 1 };
  for (let i = 0; i < n; i++) o["k" + i] = i;
  return o;
}
for (const n of [3, 1500]) {
  const o = mk(n);
  Object.defineProperty(o, "ro", { value: 1, writable: false });
  Object.defineProperty(o, "k1", { value: 7, writable: false });
  o.ro = 2;
  o["r" + "o"] = 3;
  o.k1 = 9;
  console.log(n, o.ro, o.k1, o.a, Object.keys(o).length);
  o.a = 5;
  console.log(o.a);
  const s = function (obj: any) {
    "use strict";
    try { obj.ro = 4; console.log("no throw"); } catch (e: any) { console.log(e.name); }
    try { obj.k1 = 4; console.log("no throw"); } catch (e: any) { console.log(e.name); }
  };
  s(o);
  console.log(o.ro, o.k1);
}
