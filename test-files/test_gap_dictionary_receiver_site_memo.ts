// S6 (parity read plan): a dictionary-mode receiver must never be answered
// from a per-shape memo. A receiver with more than 1,024 unique keys latches
// to dictionary mode (object/dictionary.rs) and then KEEPS its ShapeId across
// appends and some deletes that move values; any cache holding
// `(ShapeId, key) -> slot` for it answers a neighbouring slot afterwards.
// Before S6 the global read stub did exactly that: `o.x` read the value of
// `y` after `delete d.k3`, and `o.y` read the value of `x` after `delete e.y`.
function getX(o: any) { return o.x; }
function getY(o: any) { return o.y; }
function getK(o: any, k: string) { return o[k]; }
function hasX(o: any) { return "x" in o; }
function build(prefix: string): any {
  const o: any = {};
  for (let i = 0; i < 1100; i++) o[prefix + i] = i;
  return o;
}
const out: string[] = [];

const d = build("k");
d.x = 1;
d.y = 2;
let s = 0;
for (let i = 0; i < 1000; i++) s += getX(d) + getY(d) + getK(d, "x");
out.push("warm " + s);
delete d.k3;
s = 0;
for (let i = 0; i < 1000; i++) s += getX(d) + getY(d) + getK(d, "x");
out.push("after-delete " + s + " " + getX(d) + " " + getY(d) + " " + getK(d, "x"));
delete d.x;
out.push("deleted-x " + getX(d) + " " + hasX(d) + " " + getK(d, "x") + " " + getY(d));
d.x = 42;
out.push("re-added " + getX(d) + " " + hasX(d));

const e = build("q");
e.y = 7;
e.x = 9;
s = 0;
for (let i = 0; i < 1000; i++) s += getY(e) + getX(e);
out.push("e-warm " + s);
delete e.y;
out.push("e-deleted " + getY(e) + " " + getX(e) + " " + ("y" in e));

// Appends keep a dictionary's id too.
for (let i = 0; i < 50; i++) d["late" + i] = -i;
out.push("appended " + getX(d) + " " + getY(d) + " " + getK(d, "late49") + " " + Object.keys(d).length);
console.log(out.join("\n"));
