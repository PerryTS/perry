// #6924: `class MyBuf extends Buffer` — inherited statics (`MyBuf.from`,
// `.alloc`, `.isBuffer`, `.concat`) must resolve as values AND dispatch when
// invoked. The statics are BOUND_METHOD closures dispatching by name through
// the "buffer.Buffer" namespace; the per-module dispatch bucket used to be
// armed only by an explicit `buffer` import, so a program that reached Buffer
// purely through the GLOBAL (the normal case) minted the bound statics with an
// unarmed registry and every call silently returned `undefined`.
//
// Validated byte-for-byte against `node --experimental-strip-types`.

class MyBuf extends Buffer {}

console.log("typeof from:", typeof (MyBuf as any).from);
console.log("from().length:", (MyBuf as any).from("ab").length);
console.log("from() bytes:", (MyBuf as any).from("ab")[0], (MyBuf as any).from("ab")[1]);
console.log("alloc:", (MyBuf as any).alloc(3).length);
console.log("isBuffer(buf):", (MyBuf as any).isBuffer(Buffer.from("x")));
console.log("isBuffer(str):", (MyBuf as any).isBuffer("x"));
console.log(
  "concat:",
  (MyBuf as any).concat([Buffer.from("a"), Buffer.from("b")]).toString()
);

// The same statics captured as VALUES off the global (the alias shape the
// subclass read reduces to) must also dispatch.
const B: any = Buffer;
const f = B.from;
console.log("captured from:", f("cd").toString());
console.log("captured alloc:", B.alloc(2).length);
