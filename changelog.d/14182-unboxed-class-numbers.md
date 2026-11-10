Performance: loops over `number` and string-literal-union class fields and `number[]` fields on `this` keep their values in registers and their invariant work outside the loop. Measured per iteration (instructions retired, N=20M) against Node 24 on the same machine:

| probe | before | after | Node |
|---|---|---|---|
| `s += this.at(i & 127)` (method returning `this.board[sq] ?? 0`) | 65.9 | 9.0 | 15.3 |
| `s += this.board[i & 127]` | 47.2 | 9.0 | 14.9 |
| `this.hash ^= this.zob[i & 1023] ?? 0; this.rights &= …; this.rights \|= …` | 103.2 | 27.1 | 28.7 |
| `this.hash += i & 7` | 45.4 | 11.0 | 12.3 |
| `s += sign(this.side); this.side = this.side === "white" ? "black" : "white"` | 109.3 | 29.5 | 25.6 |

What changed (all behind `PERRY_UNBOXED_CLASS_FIELDS=0`):

- HIR: `this.f op= v` (and the logical assignments) reads and writes `this` in place instead of through a per-statement temp, so a loop region guards `this` once in its preheader.
- HIR: in an instance method's call-free loop, a declared, accessor-free array field `this.F` is read once before the loop. The loop may read only declared fields of `this`, the hoisted arrays and their elements. The loop is duplicated behind `ThisProvenClass(C)`, a compile-time test that is true only in a proven-`this` clone, whose receiver, layout and method table the module facts prove. A one-statement `return <pure read>` method called as `this.m(…)` is inlined into that copy, when every argument the body drops or reads twice is a literal.
- Root reloads: a re-read of a root slot with no safepoint since the previous read (or since a store of a re-read's own bits) reuses that read's launder output. A forward must-availability analysis decides this per path, after the transitive leaf-call stamping. Only the `i64` output is reused; `ROOT_RELOAD_LAUNDER` still guards every reload after a safepoint. The leaf analysis no longer counts the shadow-slot bookkeeping calls that native stack maps delete.
- Packed dense loops admit reads of raw-f64 `this` fields and int32 stores to them, fold `x ?? d` on a canonical number to `x`, and cache each validated array's element base in a slot that each fired poll re-derives.
- Strings: storing a literal the pool atomises (or a choice of two) into a field skips the addref (every atom is born shared) and the barrier's "is this a heap pointer" test; the barrier itself stays. `x === "lit"` returns false after one flags load for a non-identical property atom, and first tests identity against the other members of `x`'s literal-union type. A `this.p` field read and a strict compare against a short literal no longer force a temporary root.
- Calls: a public forwarder to a small generic body is pre-inlined like a specialized clone. A function whose every completion returns a numeric literal selection (`return c ? 1 : -1`) proves its call result a canonical Number and an integer (`returns_int`); a bare `return;` path disqualifies a function.

`PERRY_UNBOXED_CLASS_FIELDS=sabotage` is a test-only negative control that reuses reloads across safepoints. Under `PERRY_GC_SCHEDULE_SEED=… PERRY_GC_PROTECT_FROMSPACE=1` (instrumented runtime), `test_class_field_loop_gc_pressure.ts` faults with it and passes without it.
