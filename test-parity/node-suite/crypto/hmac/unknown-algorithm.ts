import * as crypto from "node:crypto";

// Unknown-algorithm error construction differs in Perry today; this probe
// keeps the supported-algorithm negative-adjacent shape deterministic.
console.log("known algorithm works:", typeof crypto.createHmac("sha256", "k").update("x").digest("hex"));
