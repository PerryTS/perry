// Dynamic-key reads, `o[k]`, through a Proxy receiver.
//
// `js_object_get_field_by_name`'s Proxy-forwarding block never satisfies the
// ordinary-object fast data-get lane above it (a Proxy's boxed encoding is a
// small registry id, not a real heap pointer), so it was pulled out into its
// own `#[inline(never)]` helper (`proxy_receiver_get`): inlined in place, the
// compiler proved the proxy registry's and the transient-handle root stack's
// `thread_local!` address resolutions were side-effect-free and hoisted BOTH
// out of the `is_proxy_id_band` guard, so an `o[k]` loop over a plain
// two-property object — never touching a Proxy — paid two `_tlv_get_addr`
// calls on every access. This exercises that the extraction is behavior
// preserving: trapped reads, pass-through reads (no `get` trap), a
// forward-to-target hop through a nested Proxy, and a Proxy loop running
// right alongside an ordinary-object loop on the same key.
const target: any = { a: 1, b: 2 };

const trapped = new Proxy(target, {
  get(t, prop, receiver) {
    if (prop === "special") return "trapped!";
    return Reflect.get(t, prop, receiver);
  },
});
for (const k of ["a", "b", "special", "missing"]) {
  console.log("trapped", k, String(trapped[k]));
}

// No `get` trap: falls through to the target's own [[Get]].
const passthrough = new Proxy(target, {});
for (const k of ["a", "b", "missing"]) {
  console.log("passthrough", k, String(passthrough[k]));
}

// Nested Proxy: a dynamic-key read that recurses through the
// forward-to-target hop inside the outlined helper.
const inner = new Proxy(target, {
  get(t, p) {
    return (t as any)[p];
  },
});
const outer = new Proxy(inner, {});
console.log("nested", outer["a"]);

// An ordinary-object loop and a Proxy loop on the same key, back to back —
// the ordinary loop must not pay for the Proxy path, and the Proxy loop must
// still resolve correctly through it.
const plain: any = { k: 1.5, other: 2 };
let plainTotal = 0;
for (let i = 0; i < 20; i++) plainTotal += plain["k"];
console.log("plain total", plainTotal);

let proxyTotal = 0;
for (let i = 0; i < 20; i++) proxyTotal += Number(trapped["a"]);
console.log("proxy total", proxyTotal);

// A Proxy over an array, read by dynamic numeric-string key.
const arrTarget = [10, 20, 30];
const arrProxy = new Proxy(arrTarget, {});
for (const k of ["0", "1", "2", "length"]) {
  console.log("arr proxy", k, String((arrProxy as any)[k]));
}
