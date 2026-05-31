// node:module createRequire / syncBuiltinESMExports / runMain parity
// (#3119, #3126, #3263). Byte-for-byte parity test against
// `node --experimental-strip-types`.
import * as module from "node:module";

// ── #3119: createRequire shape + resolve ──
const req = module.createRequire("/tmp/perry-create-require-fixture.cjs");
console.log("createRequire typeof:", typeof module.createRequire);
console.log("createRequire length:", module.createRequire.length);
console.log("require typeof:", typeof req);
console.log("require length:", req.length);
console.log("require.resolve typeof:", typeof req.resolve);
console.log("require.resolve node:fs:", req.resolve("node:fs"));
console.log("require.resolve node:path:", req.resolve("node:path"));
console.log("require.cache typeof:", typeof req.cache);
console.log("require.extensions typeof:", typeof req.extensions);

// createRequire with a file URL string is also accepted.
const reqUrl = module.createRequire("file:///tmp/perry-create-require-fixture.cjs");
console.log("createRequire(url) typeof:", typeof reqUrl);

// Invalid (non-string, non-URL) input throws ERR_INVALID_ARG_VALUE.
try {
  // @ts-ignore - intentionally calling with a number
  module.createRequire(42);
  console.log("createRequire invalid: no throw");
} catch (e) {
  console.log("createRequire invalid code:", (e as { code?: string }).code);
}

// ── #3126: syncBuiltinESMExports returns undefined, ignores extra args ──
console.log("syncBuiltinESMExports typeof:", typeof module.syncBuiltinESMExports);
console.log("syncBuiltinESMExports():", module.syncBuiltinESMExports());
console.log("syncBuiltinESMExports extra:", module.syncBuiltinESMExports(1, 2));

// ── #3263: runMain shape ──
console.log("runMain typeof:", typeof module.runMain);
console.log("runMain length:", module.runMain.length);
