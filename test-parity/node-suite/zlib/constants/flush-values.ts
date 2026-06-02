import { constants } from "node:zlib";

console.log("Z_NO_FLUSH:", constants.Z_NO_FLUSH);
console.log("Z_PARTIAL_FLUSH:", constants.Z_PARTIAL_FLUSH);
console.log("Z_SYNC_FLUSH:", constants.Z_SYNC_FLUSH);
console.log("Z_FULL_FLUSH:", constants.Z_FULL_FLUSH);
console.log("Z_FINISH:", constants.Z_FINISH);
console.log("Z_BLOCK:", constants.Z_BLOCK);
// Z_TREES is intentionally omitted: Node 22 dropped the export
// (returns undefined), while libz still defines it as 6. Cross-runtime
// parity here would just track Node's deprecation, not real behavior.
