// Issue #818 — V8-fallback class-instance return marshalling for
// `Effect.succeed(42)`-shaped patterns.
//
// `import { Thing } from "<v8-module>"; Thing.make(42)` hit the HIR's
// "uppercase Ident looks like a class" rule and lowered to
// `StaticMethodCall { class_name: "Thing", method_name: "make" }`. Pre-#818
// the codegen had no path for a Named V8 import used as a static-method
// receiver: methods.get miss, namespace_imports.contains miss, fall to
// `double_literal(0.0)` stub. Effect's `Effect.succeed(42)` produced the
// literal number 0 instead of a tagged Effect instance.
//
// Fix: when class_name is in `import_function_v8_specifiers`, route through
// the new `js_call_v8_member_method(spec, member, method, args)` runtime
// bridge. The bridge loads the module, gets `Thing` from its namespace,
// calls `.make(args)` on it, and the object return crosses back as a JS
// handle (so `inst.value` / `inst.doubled()` route through the existing
// HANDLE_PROPERTY / HANDLE_METHOD dispatch).
//
// Expected output matches `node --experimental-strip-types`:
//   object
//   42
//   Boxed
//   84
//   function
//   hello world
import { Thing } from './fixtures/v8_class_instance_pkg/v8_helper.mjs';

const inst = Thing.make(42);
console.log(typeof inst);
console.log((inst as any).value);
console.log((inst as any)._tag);
console.log((inst as any).doubled());
console.log(typeof (inst as any).pipe);
console.log(Thing.greet('world'));
