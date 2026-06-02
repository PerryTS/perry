// #1680 (Phase 2 of #1677) — a sample that validates input with a validator
// produced by `ajv/standalone` (ajv's eval-free build-time codegen). The
// generated validator is plain CommonJS (no `new Function`, no JS engine),
// so Perry compiles it natively and the binary links no runtime V8. Output
// is byte-for-byte vs `node --experimental-strip-types`.
//
// The validator here is committed (vendored); a real project declares the
// regeneration step under package.json `perry.codegen` and Perry runs it
// before compiling. See docs/src/getting-started/project-config.md.
import validate from "./ajv_user_validator.generated.cjs";

const cases: any[] = [
  { host: "localhost", port: 8080 },          // valid
  { host: "localhost" },                       // missing required `port`
  { port: 8080 },                              // missing required `host`
  { host: "a", port: 1, extra: true },         // additional property
  {},                                          // missing both
  "not-an-object",                             // wrong type
  null,                                        // wrong type
  { host: "a", port: 2, debug: false, n: 3 },  // multiple additional props
];

for (const c of cases) {
  console.log(validate(c) ? "valid" : "invalid");
}
