// #6320: a Proxy value stored where a *closure* was expected reached an ad-hoc
// CLOSURE_MAGIC probe with an `0x1000` address floor — far below
// `HANDLE_BAND_MAX` (0x100000). A proxy is a NaN-boxed registry id
// (`POINTER_TAG | (PROXY_ID_BAND_START + id)`), so the probe read `*(0xF000D +
// 12)` — unmapped low memory — and SIGSEGV'd (EXC_BAD_ACCESS at 0x000f000d).
//
// Sites: `js_to_primitive` (symbol/iterator.rs), `js_object_set_symbol_method` /
// `js_object_set_method_by_name` (symbol/properties.rs), `js_closure_unbind_this`
// (closure/dynamic_props.rs). Every line below must print and the program must
// exit 0.

const fn1 = (): string => "prim";

// --- js_to_primitive: a Proxy at [Symbol.toPrimitive] ------------------------
const obj: any = {};
obj[Symbol.toPrimitive] = new Proxy(fn1, {});
console.log("toPrimitive:", `${obj}`);

// The proxied method still sees the container as `this`, and the hint argument.
const holder: any = { value: 42 };
holder[Symbol.toPrimitive] = new Proxy(function (this: any, hint: string) {
  return hint === "number" ? this.value : "H:" + hint;
}, {});
console.log("hint string:", `${holder}`);
console.log("hint number:", +holder);
console.log("hint default:", holder + "");

// An `apply` trap intercepts the call and sees the hint in its args array.
const trapped: any = {};
trapped[Symbol.toPrimitive] = new Proxy(fn1, {
  apply(_t: any, _thisArg: any, args: any[]) {
    return "trap:" + args[0];
  },
});
console.log("apply trap string:", `${trapped}`);
console.log("apply trap number:", +trapped);

// A nested proxy forwards through both layers.
const nested: any = {};
nested[Symbol.toPrimitive] = new Proxy(new Proxy(fn1, {}), {});
console.log("nested proxy:", `${nested}`);

// A revoked proxy throws instead of crashing.
const rv = Proxy.revocable(fn1, {});
const revObj: any = {};
revObj[Symbol.toPrimitive] = rv.proxy;
rv.revoke();
try {
  console.log("revoked:", `${revObj}`);
} catch (e: unknown) {
  console.log("revoked throws TypeError:", e instanceof TypeError);
}

// --- object-literal method binding (js_object_set_*_method) ------------------
const sym: symbol = Symbol.toPrimitive;

// Computed-key method that reads `this` → js_object_set_symbol_method.
const o1: any = {
  value: "v1",
  [sym](hint: string) {
    return "m:" + hint + ":" + this.value;
  },
};
console.log("computed symbol method:", `${o1}`);

// Spread + `this`-reading methods → js_object_set_method_by_name and the
// symbol variant, both in the ordered-IIFE lowering.
const base = { a: 1 };
const o2: any = {
  ...base,
  value: "v2",
  m() {
    return "byname:" + this.value + ":" + this.a;
  },
  [sym](hint: string) {
    return "spread-sym:" + hint + ":" + this.value;
  },
};
console.log("spread method by name:", o2.m());
console.log("spread symbol method:", `${o2}`);

// A Proxy stored under a computed symbol key (a value, not a method) — it has
// no capture array to patch, so it must be stored verbatim and called through
// its [[Call]].
const pfn: any = new Proxy(function () {
  return "pv";
}, {});
const o3: any = { [sym]: pfn };
console.log("proxy computed key:", `${o3}`);

const o4: any = {
  ...base,
  [sym]: pfn,
  m() {
    return "m4:" + this.a;
  },
};
console.log("proxy after spread:", `${o4}`, o4.m());

// --- js_closure_unbind_this: detaching a proxy-valued method ----------------
const detachable: any = {
  m: new Proxy(function () {
    return "unbound";
  }, {}),
};
const g = detachable.m;
console.log("detached proxy method:", g());

console.log("END");
