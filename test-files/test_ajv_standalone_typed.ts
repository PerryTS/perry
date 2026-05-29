// #1803 (under #1680 / Phase 2 of #1677) — the per-property `type` guards in
// an `ajv/standalone`-generated validator. The structural subset (object /
// required / additionalProperties) is exercised by test_ajv_standalone.ts;
// this sample adds typed properties (string / number / boolean) so the
// generated `const _errs = errors; … if(valid0){ … }` per-property guards are
// covered. Before #1803 Perry accepted invalid typed inputs because a hoisted
// `var valid0` redeclared in each if/else branch was given a fresh slot per
// branch, so the read after the merge bound to the wrong slot. Output is
// byte-for-byte vs `node --experimental-strip-types`.
//
// The validator is committed (vendored); a real project regenerates it via the
// package.json `perry.codegen` convention. See
// docs/src/getting-started/project-config.md.
import validate from "./ajv_user_typed.generated.cjs";

const cases: any[] = [
  { name: "Ada", age: 5, active: true },   // valid
  { name: "Ada", age: 5 },                  // valid (active optional)
  { name: "Ada", age: "old" },              // age not a number  -> invalid
  { name: "Ada", age: 5, active: "yes" },   // active not boolean -> invalid
  { name: 5, age: 5 },                       // name not a string (number)  -> invalid
  { name: true, age: 5 },                    // name not a string (boolean) -> invalid
  { name: "Ada" },                           // missing required `age` -> invalid
  { age: 5 },                                // missing required `name` -> invalid
  { name: "Ada", age: 5, extra: 1 },         // additional property -> invalid
  "not-an-object",                          // wrong type -> invalid
];

for (const c of cases) {
  console.log(validate(c) ? "valid" : "invalid");
}
