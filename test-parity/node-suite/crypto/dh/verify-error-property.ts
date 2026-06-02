import * as crypto from "node:crypto";

const dh = crypto.createDiffieHellman(512);
console.log("dh verifyError:", (dh as any).verifyError);
console.log("dh verifyError own/in:", "verifyError" in dh);
const group = crypto.createDiffieHellmanGroup("modp5");
console.log("group verifyError type:", typeof (group as any).verifyError);
console.log("group verifyError own/in:", "verifyError" in group);
