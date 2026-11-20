// Issue #1698 — Request body methods (json/text/arrayBuffer) must be callable
// as a VALUE and via a COMPUTED key, not only as a direct typed call.
//
// #1688 fixed the direct typed call `req.json()` (the codegen
// `module == "Request"` arm). But `typeof req.json` reported "object",
// `req[key]` reported "undefined", and `req[key]()` returned null — because
// the value-read / computed-key forms lose the static `Request` type and land
// in the runtime's generic dynamic dispatch, which had no Request body-method
// arm. This blocks Hono's `c.req.json()` end-to-end (#1655): HonoRequest's
// `#cachedBody` does `raw[key]()` (a computed-key call on the underlying
// Request).
//
// Fix: runtime `dispatch_request_method` / `dispatch_request_property` arms
// (in `js_handle_method_dispatch` / `js_handle_property_dispatch`), a unified
// fetch-family handle id counter (so a Request id never collides with a
// Response/Headers id), and a `typeof req.json` codegen fold.
//
// Each case uses a fresh `const r = new Request(...)` var-decl: that's what
// sets `uses_fetch` so the fetch module links (an inline `new Request().m()`
// does not — see #1691), and a fresh body avoids "body already read".

// Use var-decls so the receiver type is tracked + uses_fetch is set.
const a = new Request("http://x/y", { method: "POST", headers: { "content-type": "application/json" }, body: '{"x":1}' });
console.log("typeof literal:", typeof a.json);

const b = new Request("http://x/y", { method: "POST", headers: { "content-type": "application/json" }, body: '{"x":1}' });
const kb = "json";
console.log("typeof computed:", typeof b[kb]);

const c = new Request("http://x/y", { method: "POST", headers: { "content-type": "application/json" }, body: '{"x":1}' });
const kc = "json";
console.log("typeof as-any computed:", typeof (c as any)[kc]);

async function main() {
    const d = new Request("http://x/y", { method: "POST", headers: { "content-type": "application/json" }, body: '{"x":2}' });
    const kd = "json";
    const dv = await (d as any)[kd]();
    console.log("as-any computed call x:", dv.x);

    const e = new Request("http://x/y", { method: "POST", headers: { "content-type": "application/json" }, body: '{"x":3}' });
    const ev = await (e as any).json();
    console.log("as-any direct call x:", ev.x);

    const f = new Request("http://x/y", { method: "POST", headers: { "content-type": "application/json" }, body: '{"x":4}' });
    const fv = await f.json();
    console.log("typed direct call x:", fv.x);

    const g = new Request("http://x/y", { method: "POST", body: "plain-text" });
    const kg = "text";
    console.log("computed text:", await (g as any)[kg]());
}
main();
