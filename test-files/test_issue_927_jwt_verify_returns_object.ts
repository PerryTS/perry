// Issue #927 regression: jsonwebtoken.verify must return the decoded
// payload as a parsed OBJECT, not the JSON-stringified text. The runtime
// FFI returns `*mut StringHeader` containing the JSON form; the codegen
// dispatch (NR_OBJ_FROM_JSON_STR return kind) must pipe that through
// js_json_parse so user code sees a real object — otherwise
// `decoded.sub` reads as `undefined` and auth middleware breaks.
//
// Symmetric counterpart to #915 on the return side.

import jwt from "jsonwebtoken";

function assert(condition: boolean, message: string) {
  if (!condition) {
    throw new Error(message);
  }
}

const token = jwt.sign({ sub: "u-001", acc: "a-001" }, "secret", {
  algorithm: "HS256",
});
console.log("token typeof:", typeof token);
assert(typeof token === "string", "sign should produce a string token");
assert(token.length > 0, "token should be non-empty");

const decoded = jwt.verify(token, "secret", { algorithms: ["HS256"] }) as any;

console.log("decoded typeof:", typeof decoded);
console.log("decoded.sub:", decoded.sub);
console.log("decoded.acc:", decoded.acc);

assert(typeof decoded === "object", "verify must return an object, not a string");
assert(decoded !== null, "verify must not return null on success");
assert(decoded.sub === "u-001", "decoded.sub should be 'u-001'");
assert(decoded.acc === "a-001", "decoded.acc should be 'a-001'");

console.log("issue 927 jwt.verify: ok");
