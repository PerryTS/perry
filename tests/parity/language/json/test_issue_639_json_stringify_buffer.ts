// Issue #639: JSON.stringify of a Buffer (or any value containing a Buffer)
// silently exited the process. BufferHeader has no GcHeader, so the JSON
// dispatch's `gc_obj_type(ptr)` (which reads 8 bytes before the header)
// read unrelated memory and routed to the wrong stringify arm — usually
// `is_object_pointer` deref'ing a bogus `keys_array` pointer and faulting.
//
// Surfaces in the wild as `@perryts/mysql`'s `pool.query()` result —
// `QueryResult.rowsRaw` is `RawRow[]` = `Buffer[][]`, so the result-as-a-whole
// stringify path always hit a Buffer field.

const buf = Buffer.from([1, 2, 3]);
console.log("buf alone:", JSON.stringify(buf));
console.log("[buf]:", JSON.stringify([buf]));
console.log("{b: buf}:", JSON.stringify({ b: buf }));

const u = new Uint8Array([4, 5, 6]);
console.log("u alone:", JSON.stringify(u));
console.log("[u]:", JSON.stringify([u]));
console.log("{u: u}:", JSON.stringify({ u: u }));

// Mixed: object containing arrays of buffers (mimics QueryResult.rowsRaw)
const result = {
  rowsRaw: [[Buffer.from([0x49])], [Buffer.from([0x50, 0x51])]],
  rowCount: 2,
  command: "SELECT",
};
console.log("nested:", JSON.stringify(result));

// Empty buffer
console.log("empty:", JSON.stringify(Buffer.alloc(0)));

// ──────────────────────────────────────────────────────────────────────
// Issue #639 followup: method-as-value reads on a Buffer must report
// `typeof === "function"` so duck-type tests like @perryts/mysql's
// `isBufferLike(v)` (`typeof v.readUInt8 === 'function' && typeof v.length
// === 'number'`) pass. Pre-fix every non-`length` read returned undefined
// and the npm package's prepared-statement encoder fell through to
// `String(buf)`-as-VAR_STRING, silently corrupting BLOB / BINARY columns.

function isBufferLike(v: unknown): boolean {
  if (v === null || typeof v !== "object") return false;
  const anyV = v as { readUInt8?: unknown; length?: unknown };
  return typeof anyV.readUInt8 === "function" && typeof anyV.length === "number";
}

const bb = Buffer.from([0xaa, 0xbb]);
console.log("isBufferLike(Buffer):", isBufferLike(bb));
console.log("typeof bb.readUInt8:", typeof (bb as any).readUInt8);
console.log("typeof bb.copy:", typeof (bb as any).copy);
console.log("typeof bb.toString:", typeof (bb as any).toString);
console.log("typeof bb.foo:", typeof (bb as any).foo);

// Through an Any-typed function arg (mirrors how the npm package sees it).
function check(v: any): string {
  return [
    typeof v.readUInt8,
    typeof v.writeUInt8,
    typeof v.copy,
    typeof v.length,
    typeof v.foo,
  ].join(",");
}
console.log("check:", check(bb));
