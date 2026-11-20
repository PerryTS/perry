// Regression test for #321 follow-up to #2162: the dynamic method-dispatch
// tower's default branch (`property_get.rs`'s `idispatch.default` block)
// was filling the `js_native_call_method` args buffer from the
// POST-rest-bundling `lowered_args`. When the dispatched method has a
// rest-bundled signature (e.g. effect's `pipe()` once it gained the
// synthesized `...arguments` rest from #2162), `lowered_args` had already
// been truncated+bundled to `[recv, rest_array]`; the default branch then
// alloca'd `[args.len() x double]`, stored only one value (the rest_array)
// into slot 0, and told `js_native_call_method` to read `args.len()`
// doubles — slots 1..N-1 were uninit garbage that landed in
// `pipeArguments`'s `arguments[i]`, tripping `value is not a function` at
// module-init time on a barrel `import { Effect } from "effect"`.
//
// Shape needed to trigger the fix path:
//   1. ≥1 user class with a `pipe()` method that uses `arguments` (so the
//      method has `method_has_rest` registered and rest-bundling kicks in).
//   2. The call-site receiver is NOT one of those user classes (so the
//      dispatch tower's class-id switch misses and falls into the default
//      branch).
//   3. The call has ≥2 args so the codegen alloca's a multi-slot buffer.
//
// Compared byte-for-byte against `node --experimental-strip-types`.

// (1) A user class with a Pipeable-shape pipe() method. This is enough to
// flip on the dispatch tower's class-id switch and the `method_has_rest`
// rest-bundling path for the call-site dispatch of `.pipe(...)`.
class Pipeable1 {
  v = 0;
  pipe() {
    let acc: any = this;
    for (let i = 0; i < arguments.length; i++) acc = arguments[i](acc);
    return acc;
  }
}
const _keep = new Pipeable1(); // keep the class alive

// (2) Plain object literal with a same-shape pipe() method. Its class_id
// (an anon-shape class id) is NOT Pipeable1's, so a method dispatch on it
// falls through to `idispatch.default` — the fix path.
const proto: any = {
  v: 100,
  pipe() {
    let acc: any = this;
    for (let i = 0; i < arguments.length; i++) acc = arguments[i](acc);
    return acc;
  },
};

const inc = (s: any) => ({ ...s, v: s.v + 1 });
const dbl = (s: any) => ({ ...s, v: s.v * 2 });
const sub = (s: any) => ({ ...s, v: s.v - 3 });

// (3) Three transforms = exactly the shape that triggered the
// SchemaAST_ts__282 crash inside effect:
// `getTitleAnnotation(ast).pipe(orElse, orElse, map)`. The receiver type was
// inferred wide enough that the static-call fast paths missed and the call
// landed in `idispatch.default`.
const r3 = proto.pipe(inc, dbl, sub);
console.log("r3.v:", r3.v); // ((100+1)*2)-3 = 199

const r2 = proto.pipe(inc, dbl);
console.log("r2.v:", r2.v); // (100+1)*2 = 202

const r1 = proto.pipe(inc);
console.log("r1.v:", r1.v); // 101

const r0 = proto.pipe();
console.log("r0.v:", r0.v); // 100
