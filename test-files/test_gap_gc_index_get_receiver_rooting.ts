// #7154: the BASE of a computed property READ must be rooted across the
// evaluation of the key expression.
//
// `o[f()]` evaluates the base first and the key second — spec order, and
// codegen follows it — which left the base in a bare SSA register while `f()`
// was lowered. `f()` allocates, and under `PERRY_GC_MOVING_LOOP_POLLS=1` a loop
// back-edge poll inside it runs an evacuating minor. The base SURVIVES (the
// closure capture cell / module global holding it is a root), so it MOVES: the
// collector rewrites that location but not the register. The field read then
// walks the keys array of abandoned from-space memory — a SIGSEGV inside
// `get_field_by_name_object_tail`, or a silently wrong value.
//
// This is the READ counterpart of #7192's `index_set` / `property_set` receiver
// guard, which fixed only the STORE side (`o[f()] = v`). In the registry it is
// zod's `core/checks.ts:68`,
// `numericOriginMap[typeof def.value as "number" | "bigint" | "object"]` —
// a module-global base with a key expression that reads a property, and
// therefore can collect.
//
// LIVE BY CONSTRUCTION. The base is read out of a closure capture, so it is a
// young movable object; the key expression allocates long enough that the minor
// runs EARLY in it and the abandoned from-space copy is then reused by the rest
// of the key's own allocation; and the value read back is a heap object whose
// field is dereferenced, so a stale read is observable rather than latent.
// Clean under a non-moving collector, so the evacuating arms are the ones that
// bite.

function keyOf(v: number): string {
  const bits: any[] = [];
  for (let i = 0; i < 4000; i++) {
    bits.push({ i: i, s: "x", pad: [i, i + 1, i + 2] });
  }
  return bits.length === 4000 ? "hit" : "miss";
}

function make(tag: number): (k: number) => number {
  const originMap: any = {
    hit: { v: tag, name: "hit" },
    miss: { v: -1, name: "miss" },
    other: { v: -2, name: "other" },
    spare: { v: -3, name: "spare" },
  };
  return (k: number) => (originMap[keyOf(k)] as any).v as number;
}

function run(): number {
  let bad = 0;
  for (let r = 0; r < 200; r++) {
    const f = make(r);
    const got = f(r);
    if (got !== r) {
      bad++;
    }
  }
  return bad;
}

console.log("bad", run());
