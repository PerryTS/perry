// #7280: an OPTIONAL PARAMETER must keep its shadow slot.
//
// This is the file that closes #7154's residual — the one that kept
// `sfw-registry --help` red under the loop-polls configuration and so kept
// #7161's evacuating-minor stopgap in place. Distilled from zod
// `src/v4/core/util.ts:485`:
//
//   export function clone<T>(inst: T, def?: T["_zod"]["def"], params?: { parent: boolean }): T {
//     const cl = new inst._zod.constr(def ?? inst._zod.def);
//     if (!def || params?.parent) cl._zod.parent = inst;
//     return cl as any;
//   }
//
// `params` is `Object(...)` in HIR and `type_is_pointer_bearing` says true, yet
// `collect_pointer_typed_locals` gave it no slot: it lived in callee-saved `d8`
// across `new inst._zod.constr(...)` — a user constructor crossing ~180 copying
// minors — and `params?.parent` then dereferenced retired from-space.
//
// THE CAUSE IS THE OPTIONAL MARKER, not the object type. The pointer-locals
// refinement fixpoint proves a local non-pointer from its WRITES, and for a
// PARAMETER the write list is a strict subset of its definitions — the incoming
// argument is not a write. The optional-parameter desugaring then supplies, for
// free, the one write needed to complete the false proof:
//
//     if (p === undefined) { p = undefined; }
//
// `Void` is definitely-non-pointer, so "every write is non-pointer" held, and
// the parameter lost its slot while its declared type said `Object`.
//
// TWO FUNCTIONS, TWO HALVES OF THE SAME PROOF:
//
//   `annotated`  — `params?: { parent: boolean }`, zod's exact shape. Fails on
//                  the `all_non_pointer` half alone.
//   `inferred`   — `p?: any`, plus a local aliased FROM it. Here the fixpoint's
//                  SECOND conclusion also fires: `inferred_ty` becomes `Void`,
//                  `local_value_types[p] = Void`, and the alias inherits that,
//                  is proven non-pointer, and loses ITS slot too — so the defect
//                  propagates one hop past the parameter. Measured red 200/200
//                  with only the first half applied, which is why both are here.
//
// LIVE BY CONSTRUCTION: the constructor allocates hard enough to reach the
// collector, and the optional parameter is READ only after it returns.

class Payload {
  b: number;
  constructor(n: number) {
    const bits: any[] = [];
    for (let i = 0; i < 300; i++) {
      bits.push({ i: i, s: "x" });
    }
    this.b = bits.length;
  }
}

function annotated(inst: any, def?: number, params?: { parent: boolean }): number {
  let bad = 0;
  const cl = new inst.ctor(def);
  if ((cl.b as number) !== 300) {
    bad++;
  }
  if (params !== null && params !== undefined) {
    if ((params.parent as boolean) !== true) {
      bad++;
    }
  } else {
    bad++;
  }
  return bad;
}

function inferred(inst: any, def?: number, p?: any): number {
  const alias = p;
  let bad = 0;
  const cl = new inst.ctor(def);
  if ((cl.b as number) !== 300) {
    bad++;
  }
  if (alias !== null && alias !== undefined) {
    if ((alias.parent as number) !== 7) {
      bad++;
    }
  } else {
    bad++;
  }
  return bad;
}

function runAnnotated(): number {
  let bad = 0;
  for (let r = 0; r < 150; r++) {
    bad += annotated({ ctor: Payload }, r, { parent: true });
  }
  return bad;
}

function runInferred(): number {
  let bad = 0;
  for (let r = 0; r < 150; r++) {
    bad += inferred({ ctor: Payload }, r, { parent: 7 });
  }
  return bad;
}

console.log("annotated", runAnnotated());
console.log("inferred", runInferred());
