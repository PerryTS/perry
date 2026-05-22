import { Buffer } from "node:buffer";

for (const input of ["aGk", "aGk=", "a G\nk=", "!!!!", "-_8"]) {
  try { console.log("base64:", JSON.stringify(input), Buffer.from(input, "base64").toString("hex")); } catch (err: any) { console.log("base64 err:", err?.name, err?.code || "no-code"); }
  try { console.log("base64url:", JSON.stringify(input), Buffer.from(input, "base64url").toString("hex")); } catch (err: any) { console.log("base64url err:", err?.name, err?.code || "no-code"); }
}
