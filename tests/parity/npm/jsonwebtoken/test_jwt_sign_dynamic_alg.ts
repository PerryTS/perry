// Issue #1074: `jwt.sign(payload, key, { algorithm: X, ... })` silently
// fell back to HS256 — using the user's PEM as an HMAC secret — when
// `algorithm` was anything other than an inline string literal. Mirror
// bug on `jwt.verify`'s `algorithms: [...]` option.
//
// Cases:
//   (A) inline literal — already worked.
//   (B) const-bound identifier — pre-fix: HS256 downgrade.
//   (C) entire options object is a const ref — pre-fix: HS256 downgrade.
//   (D) verify with const-bound algorithm — pre-fix: HS256 verifier
//       rejected the ES256 token.
//
// We use fixed PEMs (not `crypto.generateKeyPairSync`, which isn't
// implemented at runtime in Perry) so the test runs identically under
// `node --experimental-strip-types` and the compiled binary.

import jwt from "jsonwebtoken";

const PRIVATE_KEY = `-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgDs+eWRRv9eNsy9jl
OtgmyVnBsC3sPnHSS9CFY50PaYqhRANCAATwulSHHEW9sy6mtXYotgUxQXF/8i3d
lveYC6TEbgW+DDWNCbo5l39ck+0YBA0X3LzLYB8y9DzBii0xLisYIBNP
-----END PRIVATE KEY-----`;

const PUBLIC_KEY = `-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE8LpUhxxFvbMuprV2KLYFMUFxf/It
3Zb3mAukxG4Fvgw1jQm6OZd/XJPtGAQNF9y8y2AfMvQ8wYotMS4rGCATTw==
-----END PUBLIC KEY-----`;

const ALG = "ES256";

// (A) inline literal — was the only working path.
const tokA = jwt.sign({ sub: "a" }, PRIVATE_KEY, { algorithm: "ES256", issuer: "z" });
const headA = JSON.parse(Buffer.from(tokA.split(".")[0]!, "base64url").toString("utf8"));
console.log("(A)", headA.alg);

// (B) const ref — was falling back to HS256.
const tokB = jwt.sign({ sub: "b" }, PRIVATE_KEY, { algorithm: ALG, issuer: "z" });
const headB = JSON.parse(Buffer.from(tokB.split(".")[0]!, "base64url").toString("utf8"));
console.log("(B)", headB.alg);

// (C) const opts — was falling back to HS256.
const opts = { algorithm: "ES256" as const, issuer: "z" };
const tokC = jwt.sign({ sub: "c" }, PRIVATE_KEY, opts);
const headC = JSON.parse(Buffer.from(tokC.split(".")[0]!, "base64url").toString("utf8"));
console.log("(C)", headC.alg);

// (D) verify with const-ref algorithm.
const decoded = jwt.verify(tokA, PUBLIC_KEY, { algorithms: [ALG] }) as any;
console.log("(D)", decoded.sub);
