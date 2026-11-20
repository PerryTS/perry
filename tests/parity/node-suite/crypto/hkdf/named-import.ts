import { hkdfSync } from "node:crypto";

console.log("named hkdf:", Buffer.from(hkdfSync("sha256", "ikm", "salt", "info", 16)).toString("hex"));
