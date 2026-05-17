// Issue #915 regression: jsonwebtoken.sign must use the JWT runtime ABI
// directly. The runtime takes raw StringHeader pointers and returns an
// already-NaN-boxed string, so the generic native dispatch path used to
// segfault or double-box the token.

import jwt from "jsonwebtoken";

function assert(condition: boolean, message: string) {
  if (!condition) {
    throw new Error(message);
  }
}

const objectToken = jwt.sign({ sub: "x" }, "secret", { algorithm: "HS256" });
console.log("object token typeof:", typeof objectToken);
console.log("object token len > 0:", objectToken.length > 0);
assert(typeof objectToken === "string", "object payload token is not a string");
assert(objectToken.length > 0, "object payload token is empty");

const stringToken = jwt.sign("payload", "secret", { algorithm: "HS256" });
console.log("string token typeof:", typeof stringToken);
console.log("string token len > 0:", stringToken.length > 0);
assert(typeof stringToken === "string", "string payload token is not a string");
assert(stringToken.length > 0, "string payload token is empty");

console.log("issue 915 jwt.sign: ok");
