import * as crypto from "node:crypto";

const key = Buffer.alloc(32, 0x42);
const iv  = Buffer.alloc(12, 0x33);
const plain = Buffer.from("hello world", "utf8");

// Issue #1111 repro: optional-call on a CipherHandle's method.
// Pre-fix, c.getAuthTag (property access on a small handle) returned
// undefined, so the `?.` short-circuit fired and tag was undefined.
const c = crypto.createCipheriv("aes-256-gcm", key, iv);
const ct  = Buffer.concat([c.update(plain), c.final()]);
const tag = (c as any).getAuthTag?.();
console.log("ct:", ct.toString("hex"));
console.log("tag:", tag?.toString?.("hex") ?? "(none)");

// Method-as-value: the bound closure should report as `function`.
console.log("typeof getAuthTag:", typeof (c as any).getAuthTag);

// Full roundtrip via decipher.setAuthTag.
const d = crypto.createDecipheriv("aes-256-gcm", key, iv);
if (tag) (d as any).setAuthTag(tag);
const out = Buffer.concat([d.update(ct), d.final()]);
console.log("decrypted:", out.toString("utf8"));
