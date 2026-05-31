import crypto from "node:crypto";

function report(label: string, fn: () => unknown): void {
  try {
    const r = fn();
    const shown = Buffer.isBuffer(r) ? `buf:${r.length}` : String(r);
    console.log(label, "ok", shown);
  } catch (e: any) {
    console.log(label, "err", e.name, e.code, JSON.stringify(e.message));
  }
}

// --- #2944: Hash finalized state ---------------------------------------
const h = crypto.createHash("sha256");
h.update("abc");
console.log("hash first", h.digest("hex"));
report("hash digest-again", () => h.digest("hex"));
report("hash update-after", () => h.update("x"));

// --- #2945: Hmac finalized state ---------------------------------------
const hm = crypto.createHmac("sha256", "secret-key");
hm.update("abc");
console.log("hmac first", hm.digest("hex"));
report("hmac digest-again", () => hm.digest("hex"));
report("hmac update-after", () => hm.update("x"));

// --- #2962: Cipher / Decipher invalid state ----------------------------
const key = Buffer.from("000102030405060708090a0b0c0d0e0f", "hex");
const iv = Buffer.from("0f0e0d0c0b0a09080706050403020100", "hex");
const c = crypto.createCipheriv("aes-128-cbc", key, iv);
c.update("abc");
console.log("cipher final1", c.final().toString("hex"));
report("cipher final2", () => c.final());
report("cipher update-after-final", () => c.update("x"));
report("cipher setAutoPadding-after-final", () => c.setAutoPadding(false));

const ivg = Buffer.from("0102030405060708090a0b0c", "hex");
const g = crypto.createCipheriv("aes-128-gcm", key, ivg);
report("gcm getAuthTag-before-final", () => g.getAuthTag());
g.update("abc");
report("gcm setAAD-after-update", () => g.setAAD(Buffer.from("aad")));
g.final();
report("gcm getAuthTag-after-final", () => g.getAuthTag());
