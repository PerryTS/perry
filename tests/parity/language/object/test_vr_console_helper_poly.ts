// Owner ruling 2026-11-10 (DESIGN-p4l4 §4.4): a logging helper whose
// parameter joins a record with other values is Boxed; the record is
// materialized at the call and stays a value record. `show` is
// self-recursive so that the inliner leaves the calls in place.
// vr-expect: from,to ADMITTED
function mk(f: number, t: number) {
  return { from: f, to: t };
}
function show(x: unknown, depth: number): void {
  if (depth > 0) {
    show(x, depth - 1);
    return;
  }
  console.log(x);
}
const c = Math.random() > 2;
const u = mk(8, 16);
show(c ? 5 : u, 1);
show(u, 0);
show("text", 0);
show(5, 0);
console.log(u.from + u.to);
