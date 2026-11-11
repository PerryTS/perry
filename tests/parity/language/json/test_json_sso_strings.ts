// Regression test for v0.5.277: JSON.parse-emitted SSO strings
// (≤5-byte strings, encoded inline via SHORT_STRING_TAG since
// v0.5.216) must work in every consumer path that previously
// only handled heap STRING_TAG. Pre-fix `JSON.parse(...).foo`
// printed `NaN` and `=== "literal"` returned false because the
// SSO bit pattern (0x7FF9...) IS a NaN double and the print /
// equality paths fell through to the regular-number branch.

const a = JSON.parse('{"foo":"perry"}');
console.log("a.foo:", a.foo);

// Single-arg + multi-arg console.log paths.
const v = a.foo;
console.log(v);
console.log("v=", v, "/end");

// SSO + heap-string mixed equality (=== / ==).
const u: any = "perry";
console.log("v === u:", v === u);
console.log("v == u:", v == u);
console.log('v === "perry":', v === "perry");

// Coercion paths: String(v), "" + v, v.toString().
console.log("String(v):", String(v));
console.log('"X" + v:', "X" + v);

// SSO inside a nested object literal printed via util.inspect.
const wrapper = JSON.parse('{"name":"alice","tag":"vip"}');
console.log(wrapper);

// Array of SSO strings printed via spread.
const arr = JSON.parse('["a","bb","ccc","dddd","eeeee"]');
console.log(arr);
console.log("arr.length:", arr.length);

// SSO key in Object.keys (keys themselves go through interning,
// but the values are SSO; Object.keys returns the keys array).
console.log("keys:", Object.keys(a));

// JSON.stringify roundtrip — already covered by other tests but
// keep a line here so a future regression in stringify shows up
// alongside the SSO failures.
console.log("stringify:", JSON.stringify(a));
