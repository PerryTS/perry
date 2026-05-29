// Issue #2013 — Node-shaped argument validation for `crypto.randomBytes`
// and `crypto.randomInt`. Each probe prints the thrown error's `.code`
// and `.name`; Perry and Node must produce the exact same lines. The
// other crypto entry points (createHash / createHmac / pbkdf2Sync /
// …) take string args that perry-stdlib's runtime currently receives
// already-unboxed as i64; their validation needs a parallel
// `_jsv`-shaped entry that's the natural follow-up.
import * as crypto from "node:crypto";

function probe(label: string, fn: () => any) {
  try {
    fn();
    console.log(label, "no-throw");
  } catch (e: any) {
    console.log(label, e.name, e.code);
  }
}

// randomBytes — non-number type and out-of-range integers.
probe("randomBytes({})", () => crypto.randomBytes({} as any));
probe("randomBytes('abc')", () => crypto.randomBytes("abc" as any));
probe("randomBytes(true)", () => crypto.randomBytes(true as any));
probe("randomBytes(null)", () => crypto.randomBytes(null as any));
probe("randomBytes(-1)", () => crypto.randomBytes(-1));
probe("randomBytes(1.5)", () => crypto.randomBytes(1.5));
probe("randomBytes(NaN)", () => crypto.randomBytes(NaN));
probe("randomBytes(Infinity)", () => crypto.randomBytes(Infinity));
probe("randomBytes(2**31)", () => crypto.randomBytes(2 ** 31));

// randomInt — same shape on either bound.
probe("randomInt({}, 10)", () => crypto.randomInt({} as any, 10));
probe("randomInt(0, {})", () => crypto.randomInt(0, {} as any));
probe("randomInt('abc', 10)", () => crypto.randomInt("abc" as any, 10));
probe("randomInt(0, 1.5)", () => crypto.randomInt(0, 1.5));
probe("randomInt(NaN, 10)", () => crypto.randomInt(NaN, 10));
