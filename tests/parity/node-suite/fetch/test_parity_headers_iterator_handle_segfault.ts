// Regression: js_get_iterator / is_builtin_iterator_class_id must not
// dereference a Web-Fetch handle (Headers / Request.headers / Response.headers)
// as a heap object.
//
// A `new Headers()` (and the Headers exposed by Request/Response) is a
// NaN-boxed small handle id in [0x40000, 0x100000), NOT a heap pointer. #4786
// switched the `for...of` lowering for opaque (non-Array/Map/Set/string)
// iterables from the eager `js_for_of_to_array` path to the lazy iterator
// protocol (`GetIterator(obj)` -> `js_get_iterator`). `js_get_iterator` calls
// `is_builtin_iterator_class_id` to short-circuit values that are already
// iterators; pre-fix that helper floored its pointer-validity check at 0x1008,
// so the 0x40000+ Headers handle sailed past the guard and the runtime
// dereferenced `[handle - GC_HEADER_SIZE]` as a GcHeader -> SIGSEGV. This
// crashed every Hono HTTP response on Linux x86_64 — `@hono/node-server`s
// `buildOutgoingHttpHeaders` does `for (const [k, v] of headers)` on the
// response Headers — so even GET /healthz segfaulted. macOS arm64 masked it
// via a higher heap floor.
//
// Expected: prints PASS and exits 0. Pre-fix: SIGSEGV (exit 139), no "PASS".

// Launder through `any` so codegen cannot prove the static type and must route
// `for...of` / spread through the generic runtime iterator (the crashing path).
function asAny<T>(x: T): any {
  return x;
}

// Mirrors Hono`s buildOutgoingHttpHeaders: `for (const [k, v] of headers)` over
// a value codegen sees as opaque, with array-destructuring of each pair.
function entriesToString(it: any): string {
  const parts: string[] = [];
  for (const [k, v] of it) parts.push(`${k}=${v}`);
  return parts.join(",");
}

let failures = 0;
function expect(label: string, got: string, want: string): void {
  if (got !== want) {
    console.log(`FAIL ${label}: got=${got} want=${want}`);
    failures++;
  }
}

// 1. for...of (array-destructuring) over a Headers handle.
expect(
  "headers-for-of",
  entriesToString(asAny(new Headers([["content-type", "text/plain"], ["x-a", "1"]]))),
  "content-type=text/plain,x-a=1",
);

// 2. spread over a Headers handle.
{
  const arr = [...asAny(new Headers([["a", "1"], ["b", "2"]]))] as [string, string][];
  expect("headers-spread", arr.map((e) => `${e[0]}=${e[1]}`).join(","), "a=1,b=2");
}

// 3. Response.headers — the exact @hono/node-server `buildOutgoingHttpHeaders`
//    path (the headers handle is allocated lazily off the Response).
{
  const r = new Response("hi", {
    headers: { "content-type": "text/plain", "x-z": "9" },
  });
  expect("response-headers", entriesToString(asAny(r.headers)), "content-type=text/plain,x-z=9");
}

// 4. Request.headers — same Headers-handle representation, server-request side.
{
  const req = new Request("http://example.test/", {
    headers: { "content-type": "application/json", "x-q": "7" },
  });
  expect("request-headers", entriesToString(asAny(req.headers)), "content-type=application/json,x-q=7");
}

if (failures === 0) {
  console.log("PASS");
} else {
  console.log("FAIL: " + failures + " case(s) failed");
}
