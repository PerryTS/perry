// Regression: js_object_get_own_field_or_undef must not dereference a
// Web-Fetch handle (Headers/Request/Response/Blob) as a heap object.
//
// A `new Headers()` is a NaN-boxed small handle id in [0x40000, 0x100000),
// not a heap pointer. When a user class in the same program declares a
// method whose name collides with a Headers method (here `.set`), the
// codegen dynamic-dispatch tower fires an own-property override probe
// (`js_object_get_own_field_or_undef`) on the receiver before dispatch.
// Pre-fix, that probe floored pointer validation at 0x10000, so the
// 0x40000 Headers handle slipped through and the runtime dereferenced
// `[handle - GC_HEADER_SIZE]` -> SIGSEGV (crashed every Hono HTTP
// response on Linux x86_64; macOS masked it via a higher heap floor).
//
// A user class that ALSO has `.set` / `.delete` is what makes the call
// site polymorphic and routes `headers.set(...)` through the override
// probe instead of a direct Headers native dispatch.
class Updater {
  v: number = 0;
  set(x: number): number { this.v = x; return this.v; }
  delete(x: number): number { this.v -= x; return this.v; }
}

function exercise(): string {
  const u = new Updater();
  u.set(41);
  u.delete(1);

  const h = new Headers();
  // These dispatch through the same `.set` / `.delete` / `.append` tower
  // and would segfault pre-fix when probing the Headers handle.
  h.set("content-type", "application/json");
  h.append("x-extra", "1");
  h.set("x-extra", "2");
  h.delete("x-extra");

  const ct = h.get("content-type") ?? "MISSING";
  return `${u.v}:${ct}`;
}

const result = exercise();
if (result === "40:application/json") {
  console.log("PASS");
} else {
  console.log("FAIL got=" + result);
}
