// Refs #2128: property access on a primitive number value via the generic
// runtime field-getter must return `undefined`, not SIGSEGV.
//
// Pre-fix, `js_object_get_field_by_name` had no guard for a finite-double
// receiver (top16 not in 0x7FF9..=0x7FFF, top16 != 0) — codegen would
// bit-cast the f64 to a pointer and the first downstream helper that read
// a GC header (`is_registered_set`, `(*obj).field_count`, etc.) derefed
// unmapped memory. drizzle-orm's `buildQueryFromSourceParams` surfaces this
// when it maps over SQL chunks that include bound-param numbers (an `id`,
// an `age` etc.); UPDATE / DELETE / leftJoin all crashed with rc=139.
//
// JS spec: property access on a primitive number returns `undefined` for
// unknown keys (auto-boxing to `Number.prototype` is a separate path for
// known methods). The runtime field-getter slow path now returns undefined
// instead of dereferencing.

// Lose static type info so codegen can't inline a number-aware fast path.
function asAny(v: number): any { return v; }

const n = asAny(1);
console.log("number .foo:", n.foo);

const z = asAny(0);
console.log("zero .anything:", z.anything);

// Negative + float — different f64 bit patterns, same guard.
console.log("negative .x:", asAny(-3.14).x);
console.log("float .y:", asAny(2.5).y);

// Property access on the result of an `any` array index.
const arr: any[] = [42];
console.log("arr[0].x:", arr[0].x);

// JSON.parse("1") returns a number typed as any — load-bearing for the
// drizzle-style chunk-mapping pattern.
const j: any = JSON.parse("1");
console.log("json-number .y:", j.y);
