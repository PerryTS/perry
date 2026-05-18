// Issue: V8 namespace-import member call returns 0 / undefined.
//
// `import * as ns from "<v8-module>"; ns.member(args)` was reaching the
// codegen's StaticMethodCall arm (because `ns` is uppercase-or-imported,
// HIR lifts it to a static-method-style call) but no V8 specifier was
// registered for `ns.member`, so the call fell through to the
// `double_literal(0.0)` stub. Affected ramda (`R.sum([1,2,3,4,5]) === 0`
// instead of `15`), and any other consumer that uses a wildcard
// namespace import to access a function in a V8-fallback module.
//
// Fix: `compile.rs` now registers each V8 namespace local under
// `namespace_v8_specifiers: local → specifier`, and the codegen
// StaticMethodCall + namespace-member-call arms probe that map before
// falling through to the native-prefix lookup.
//
// Expected output matches `node --experimental-strip-types`:
//   6
//   number
//   hello world
//   string
//   42
//   84
import * as helper from './fixtures/v8_namespace_call_pkg/v8_helper.mjs';
console.log(helper.sum([1, 2, 3]));
console.log(typeof helper.sum([1, 2, 3]));
console.log(helper.greet('world'));
console.log(typeof helper.greet('world'));
const obj = helper.make(42);
console.log(obj.value);
console.log(obj.doubled());
