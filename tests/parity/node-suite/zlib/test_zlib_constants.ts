// Regression test for #955 follow-up — `zlib.constants` exposes the
// ~50 Z_*/DEFLATE/INFLATE/BROTLI_*/ZSTD_* table Node's `node:zlib`
// module ships. Axios's stream wiring reads these directly; pre-fix
// the access tripped the strict-API gate (#463) at compile time.
import { constants } from "node:zlib";

console.log(constants.Z_NO_COMPRESSION);
console.log(constants.Z_BEST_SPEED);
console.log(constants.Z_BEST_COMPRESSION);
console.log(constants.Z_DEFAULT_COMPRESSION);
console.log(constants.Z_DEFAULT_STRATEGY);
console.log(constants.Z_FINISH);
console.log(constants.Z_OK);
console.log(constants.DEFLATE);
console.log(constants.INFLATE);
console.log(constants.GZIP);
console.log(constants.BROTLI_DEFAULT_QUALITY);
