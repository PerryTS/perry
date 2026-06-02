// Issue #1830: SIGSEGV from an explicit gc() inside a catch block when the
// thrown exception propagated up from a DEEPER function that had pointer-typed
// locals.
//
// Root cause: js_throw uses longjmp to unwind, which skips the intervening
// functions' epilogues — and therefore their js_shadow_frame_pop calls. The
// shadow stack's frame_top is left pointing at the orphaned (now-dead) callee
// frames. A GC in the catch body then walks those frames, reads slot pointers
// into dead stack memory, and tries to evacuate a garbage "pointer".
//
// gc() is guarded with `typeof gc === "function"` so this file also runs under
// `node --experimental-strip-types` (Node does not expose gc() by default), and
// the byte-for-byte output matches Perry.

declare const gc: (() => void) | undefined;

function deep3(): number {
  const a = { x: 1, y: 2, tag: "deep3-obj" };
  const b = [a, a, a, a];
  const c = "deep3-" + String(b.length) + "-" + a.tag;
  if (c.length > 0) {
    throw new Error("boom from deep3 len=" + String(c.length));
  }
  return b.length;
}

function deep2(): number {
  const arr = [1, 2, 3, 4, 5].map((n) => ({ n, label: "d2-" + String(n) }));
  const s = { tag: "deep2", items: arr };
  return deep3() + arr.length + s.items.length;
}

function deep1(): number {
  const s = { tag: "d1", data: [10, 20, 30], note: "level-one" };
  const more = s.data.map((v) => "v" + String(v));
  return deep2() + s.data.length + more.length;
}

function run(): void {
  try {
    deep1();
  } catch (e) {
    // Orphaned shadow frames from deep1/deep2/deep3 are live here. Allocate
    // heavily to create GC pressure and reuse the dead stack region, then force
    // a collection while frame_top is (pre-fix) corrupted.
    const acc: { i: number; s: string }[] = [];
    for (let i = 0; i < 50000; i++) {
      acc.push({ i, s: "x" + String(i) });
    }
    if (typeof gc === "function") {
      gc();
    }
    const msg = (e as Error).message;
    console.log("caught:", msg, "acc=", acc.length);
  }
}

run();
console.log("done");
