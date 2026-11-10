// Gap test for #10759 — `URLSearchParams.prototype` had no entry in
// `populate_builtin_prototype_methods` (crates/perry-runtime/src/object/
// global_this/proto_methods.rs), unlike every neighboring builtin (`Headers`,
// `URLPattern`, `Request`/`Response`, ...). Its prototype methods were never
// installed as real (name-carrying, callable) property VALUES, so any read
// of `URLSearchParams.prototype.<method>` — literal, computed by a runtime
// string, or through a Proxy `get` trap — returned `undefined` instead of a
// function, and `.call()`/`.apply()` on that then threw:
//   TypeError: Function.prototype.call was called on a value that is not a
//   function
//
// This is the exact shape node-fetch@3.3.2's `Headers` class hits on EVERY
// `fetch()` call: `class Headers extends URLSearchParams` returns
// `new Proxy(this, { get(target, p, receiver) { ... return (...) =>
// URLSearchParams.prototype[p].call(target, ...); } })` from its
// constructor, and `getNodeRequestOptions()` calls `headers.has('Accept')`
// unconditionally before the request is even sent.
//
// Same underlying mechanism as the sibling fixes already in-repo for other
// builtins: `require('stream').prototype` (object/native_module/
// constants.rs) and `Function.toString`/`Object.hasOwnProperty` as values
// (crates/perry/tests/issue_5135_proxy_compound_and_function_tostring.rs) —
// a built-in prototype's methods must be installed as real closures, or a
// value-read misses regardless of how the read is spelled.
// Byte-identical to `node --experimental-strip-types` (v26.5.1).

// ---- typeof / name / length: literal, literal-string, and computed-by-variable access ----
{
  const methods = [
    "append",
    "delete",
    "entries",
    "forEach",
    "get",
    "getAll",
    "has",
    "keys",
    "set",
    "sort",
    "toString",
    "values",
  ] as const;
  const proto: any = URLSearchParams.prototype;
  const out: string[] = [];
  for (const m of methods) {
    const literal = typeof proto[m];
    const literalStr = typeof proto[m as string];
    const k: string = m;
    const computed = typeof proto[k];
    out.push(`${m}:${literal},${literalStr},${computed},len=${proto[m].length},name=${proto[m].name}`);
  }
  console.log("typeof-suite:", out.join(" "));
}

// ---- direct `.call()` on the literal-read method, mutating a real receiver ----
{
  const usp = new URLSearchParams();
  URLSearchParams.prototype.append.call(usp, "a", "1");
  console.log("literal-call:", usp.toString());
}

// ---- `.call()` through a runtime-variable computed key (the exact shape
// node-fetch's Headers.js uses inside its Proxy trap) ----
{
  const usp = new URLSearchParams();
  const k = "append";
  (URLSearchParams.prototype as any)[k].call(usp, "a", "1");
  const k2 = "has";
  const hasA = (URLSearchParams.prototype as any)[k2].call(usp, "a");
  const hasB = (URLSearchParams.prototype as any)[k2].call(usp, "b");
  console.log("computed-call:", usp.toString(), hasA, hasB);
}

// ---- the node-fetch `Headers` shape itself: a subclass whose constructor
// returns a Proxy wrapping `this`, whose `get` trap reads
// `URLSearchParams.prototype[p]` by a closure-captured (not literal)
// variable and calls it with `.call(target, ...)`. ----
class FetchLikeHeaders extends URLSearchParams {
  constructor() {
    super();
    const target: any = this;
    // eslint-disable-next-line no-constructor-return
    return new Proxy(target, {
      get(target: any, p: any, receiver: any) {
        switch (p) {
          case "append":
          case "set":
            return (name: string, value: string) => {
              return (URLSearchParams.prototype as any)[p].call(target, name, value);
            };
          case "delete":
          case "has":
          case "getAll":
            return (name: string) => {
              return (URLSearchParams.prototype as any)[p].call(target, name);
            };
          default:
            return Reflect.get(target, p, receiver);
        }
      },
    });
  }
}
{
  // Deliberately exercises only the trap's explicitly-handled cases
  // (append/set/delete/has/getAll) — the same subset node-fetch's real
  // Headers.js switch covers. Its `.get()`/`.toString()` are separate own
  // CLASS METHODS that delegate to `getAll` rather than falling through the
  // trap's `default: Reflect.get(target, p, receiver)` arm, because Node's
  // native `URLSearchParams.prototype.get`/`.toString`, called with `this`
  // bound to the Proxy receiver (as `default` would do), rejects a Proxy
  // `this` via its own internal-slot brand check — a genuine, unrelated
  // Node quirk this fixture avoids by construction, not a Perry gap.
  const headers: any = new FetchLikeHeaders();
  console.log("headers-before-has-accept:", headers.has("Accept"));
  headers.set("Accept", "*/*");
  console.log("headers-after-has-accept:", headers.has("Accept"));
  headers.append("X-Extra", "1");
  headers.append("X-Extra", "2");
  console.log("headers-getall:", headers.getAll("X-Extra").join(","));
  headers.delete("X-Extra");
  console.log("headers-after-delete:", headers.has("X-Extra"));
}

// ---- Object.prototype methods must also be present on URLSearchParams.prototype
// (installed alongside the URLSearchParams-specific set, same as every other
// builtin's arm in populate_builtin_prototype_methods). ----
{
  const proto: any = URLSearchParams.prototype;
  console.log(
    "object-proto-methods:",
    typeof proto.hasOwnProperty,
    typeof proto.isPrototypeOf,
    typeof proto.propertyIsEnumerable,
  );
}
