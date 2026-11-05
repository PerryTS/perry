// #11430: crypto entry points must accept a SHORT (SSO) string argument.
// Since #10762 `String(n)` / `${n}` / `n.toString()` return small values as
// SSO strings, whose characters live inline in the NaN-box. The crypto call
// sites unboxed every string argument with `bits & POINTER_MASK`, turning
// those inline characters into an address that `bytes_from_ptr` dereferenced:
// `hash.update(String(7))`, an HMAC key, a pbkdf2 password or a cipher's
// update input written this way segfaulted.
import { createHash, createHmac, pbkdf2Sync, createCipheriv, createDecipheriv, hkdfSync } from "node:crypto";

console.log("sha256:", createHash("sha256").update(String(7)).digest("hex"));
console.log("sha256 enc:", createHash("sha256").update(String(7), "utf8").digest("hex"));
console.log("md5 tpl:", createHash("md5").update(`${42}`).digest("hex"));
console.log("chained:", createHash("sha1").update(String(1)).update((2).toString()).digest("base64"));
console.log("hmac sso key:", createHmac("sha256", String(1)).update(String(123)).digest("hex"));
console.log("hmac heap key:", createHmac("sha256", "key").update(String(123)).digest("hex"));
console.log("pbkdf2:", pbkdf2Sync(String(9), String(8), 10, 16, "sha256").toString("hex"));
console.log("hkdf:", Buffer.from(hkdfSync("sha256", String(1), String(2), String(3), 16)).toString("hex"));
const key = Buffer.alloc(32, 1);
const iv = Buffer.alloc(16, 2);
const c = createCipheriv("aes-256-cbc", key, iv);
const enc = Buffer.concat([c.update(String(5)), c.final()]);
const d = createDecipheriv("aes-256-cbc", key, iv);
console.log("aes:", enc.toString("hex"), Buffer.concat([d.update(enc), d.final()]).toString());
