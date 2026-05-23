// crypto.subtle is the WebCrypto SubtleCrypto instance. Perry used to
// read it as a number (the 0 sentinel), so feature-detection like
// `if (crypto.subtle) { crypto.subtle.digest(...) }` saw `0` (falsy)
// and the polyfill-fallback path silently took over. Regression cover
// for #1366. Asserts shape only (typeof === "object" + truthy).
import * as crypto from "node:crypto";
console.log("subtle typeof:", typeof crypto.subtle);
console.log("subtle truthy:", !!crypto.subtle);
