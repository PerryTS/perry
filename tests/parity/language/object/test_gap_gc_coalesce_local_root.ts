// `const masks = opts?.masks ?? null` lost its GC root.
//
// Reduced from a Three.js world builder (`Accum.add`): an array-valued option
// is read through `?.` + `??` into a local before a long allocating loop and
// indexed inside it. Under the moving nursery the compiled program died with a
// from-space read on the first copying minor — SIGBUS under
// `PERRY_GC_PROTECT_FROMSPACE=1`, garbage colours or a crash otherwise.
//
// WHAT IT CATCHES. Optional chaining lowers `opts?.masks` to an `Any`-typed
// conditional, and the `??` inference rule answered the RIGHT operand's type
// for an unknown left, so the binding was declared `Null`. The codegen pointer
// analysis (`collectors/pointer_locals.rs`) took that structural inference as
// proof the local holds no pointer and gave it no shadow slot: the array's
// NaN-boxed address sat in a plain `alloca double` across the loop back-edge
// poll, the copying minor moved the array, and `masks[0]` dereferenced
// from-space. An explicit `number[] | null` annotation does NOT help — the
// collector distrusts declared types by design (#7846) — so the witness keeps
// the untyped form. A plain ternary (`opts === null ? null : opts.masks`) was
// rooted all along, which is how the `??` path was isolated.
//
// LIVE BY CONSTRUCTION: every iteration pushes into two arrays and allocates
// an object and an array, so the loop poll collects many times while `masks`
// and `paint` are live. The second arm holds a CLOSURE through the same `??`
// shape and calls it after the polls — the form in which the defect surfaced
// first (a helper's object result arriving as `undefined`). Compared
// byte-for-byte against Node.

class Accum {
  pos: number[] = [];
  col: number[] = [];
  keep: Array<{ i: number; a: number[] }> = [];

  add(
    count: number,
    opts: { masks: number[]; paint: (i: number) => number } | null,
  ): number {
    const masks = opts?.masks ?? null;
    const paint = opts?.paint ?? null;
    let painted = 0;
    for (let i = 0; i < count; i++) {
      this.pos.push(i, i + 0.25, i + 0.5);
      if ((i & 4095) === 0) {
        this.keep = [];
      }
      this.keep.push({ i, a: [i, i + 1, i + 2] });
      let r = 0;
      let g = 0;
      let b = 0;
      if (masks) {
        r = Math.max(r, masks[0]);
        g = Math.max(g, masks[1]);
        b = Math.max(b, masks[2]);
      }
      if (paint) {
        painted += paint(i);
      }
      this.col.push(r, g, b);
    }
    return painted;
  }
}

const masks = [0.35, 0.25, 0.1];
const accum = new Accum();
const painted = accum.add(200_000, { masks, paint: (i) => i & 1 });
const last = accum.col.length - 3;
console.log(
  accum.col.length,
  accum.col[last],
  accum.col[last + 1],
  accum.col[last + 2],
  painted,
  masks[0],
);
