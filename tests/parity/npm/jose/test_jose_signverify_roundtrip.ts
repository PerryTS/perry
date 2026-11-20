// Issue #819: end-to-end sign + verify roundtrip through the V8 fallback.
//
// Pre-fix: `js_async_step_chain(value, step)` checked
// `is_definitely_primitive(value)` which only treated POINTER_TAG values
// as non-primitive. A V8 Promise crossing the bridge comes back as a
// JS_HANDLE_TAG (0x7FFB) value, which slipped through the primitive
// check — the dispatch then enqueued the unresolved V8 Promise handle
// as the resolution value for the next async step, so `jwt` in user
// code observed `[object Promise]` instead of the signed JWT string.
// jose's `jwtVerify(jwt, key)` then threw `JWSInvalid: Compact JWS
// must be a string or Uint8Array`.
//
// The fix routes the value through `adapt_foreign_promise_value`
// before the primitive check, which calls back into the registered
// jsruntime adapter to wrap the V8 Promise in a native pending Promise
// the rest of the dispatch already handles correctly.
//
// This mirrors `/tmp/perry-compat-sweep/jose/entry.ts` from the
// compat sweep; expected output is `sub=alice`.
import { SignJWT, jwtVerify } from "jose";

async function main() {
    const secret = new TextEncoder().encode("test-secret-key-32-bytes-aaaaaaaa");
    const jwt = await new SignJWT({ sub: "alice" })
        .setProtectedHeader({ alg: "HS256" })
        .setExpirationTime("1h")
        .sign(secret);
    const { payload } = await jwtVerify(jwt, secret);
    console.log("sub=" + payload.sub);
}
main();
