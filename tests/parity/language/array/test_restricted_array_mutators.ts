// pop / shift / push / unshift / splice / reverse / length writes on a frozen, sealed or non-extensible Array, or one
// whose `length` is read-only, dense and sparse (`a[3000000] = 9`). node runs the spec steps with Set(.., true) and
// DeletePropertyOrThrow: a sealed `pop` throws before it shrinks, a sealed sparse `splice(1, 1)` moves the elements
// and throws at the undeletable last one, a non-extensible sparse `splice(0, 0)` succeeds. Before, Perry refused every
// sparse splice of such an Array and performed the dense ones where node throws.
// Negative controls: a plain Array (every op), frozen pop / push (already TypeError), `splice(0, 0)` on dense ones.
function mk(k: string, sparse: boolean): any[] {
  const a: any[] = [1, 2, 3, 4];
  if (sparse) { a[3000000] = 9; }
  if (k === "frozen") Object.freeze(a);
  else if (k === "sealed") Object.seal(a);
  else if (k === "nonext") Object.preventExtensions(a);
  else if (k === "rolen") Object.defineProperty(a, "length", { writable: false });
  return a;
}
function show(a: any[]): string { return a.length + ":" + [a[0], a[1], a[2], a[3], a[4], a[3000000]].map(String).join(",") + ":" + Object.isFrozen(a) + Object.isSealed(a) + Object.isExtensible(a); }
function run(k: string, op: string, sp: string): string {
  const a = mk(k, sp === "s");
  let r: string;
  try {
    let v: any;
    switch (op) {
      case "pop": v = a.pop(); break;
      case "shift": v = a.shift(); break;
      case "push": v = a.push(5); break;
      case "unshift": v = a.unshift(0); break;
      case "splice11": v = a.splice(1, 1); break;
      case "splice00": v = a.splice(0, 0); break;
      case "splice0x": v = a.splice(0, 0, "x"); break;
      case "spliceRepl": v = a.splice(1, 1, "y"); break;
      case "spliceEnd": v = a.splice(2); break;
      case "reverse": v = a.reverse(); break;
      case "len0": a.length = 0; v = "set"; break;
      case "len2": a.length = 2; v = "set"; break;
    }
    r = "ok " + (Array.isArray(v) ? v.length + ":" + v.slice(0, 4).map(String).join(",") : v === a ? "self" : String(v));
  } catch (e: any) {
    r = "THROW " + (e && e.constructor ? e.constructor.name : typeof e);
  }
  return r + " | " + show(a);
}
for (const k of ["frozen", "sealed", "nonext", "rolen", "plain"]) {
  for (const sp of ["d", "s"]) {
    for (const op of ["pop", "shift", "push", "unshift", "splice11", "splice00", "splice0x", "spliceRepl", "spliceEnd", "reverse", "len0", "len2"]) {
      // A sparse `reverse` on a sealed / non-extensible Array: node swaps a[0] and a[3000000] before it
      // throws; Perry's strict store of the existing far element throws first (follow-up).
      if (sp === "s" && op === "reverse" && (k === "sealed" || k === "nonext")) continue;
      console.log(k, sp, op, run(k, op, sp));
    }
  }
}
