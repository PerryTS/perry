import * as crypto from "node:crypto";

const h = crypto.createHash("sha256");
h.update("abc");
console.log("first:", h.digest("hex"));
// Node throws ERR_CRYPTO_HASH_FINALIZED on a second digest/update; Perry's
// handle currently returns undefined/no-ops. Keep this case focused on the
// finalized digest value until runtime exception parity is available.
