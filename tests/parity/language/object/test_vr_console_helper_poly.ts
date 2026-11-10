// Owner ruling 2026-11-10 (DESIGN-p4l4 §4.4): a logging helper whose
// parameter joins a record with other values is Boxed; the record is
// materialized at the call and stays a value record.
// vr-expect: from,to ADMITTED
function mk(f: number, t: number) {
  return { from: f, to: t };
}
function show(x: unknown) {
  console.log(x);
}
const c = Math.random() > 2;
const u = mk(8, 16);
show(c ? 5 : u);
show(u);
show("text");
show(5);
console.log(u.from + u.to);
