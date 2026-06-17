import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

function report(label: string, fn: () => unknown) {
  try {
    const value = fn();
    console.log(label, "ok", Buffer.isBuffer(value) ? `buffer:${value.length}` : typeof value);
  } catch (err: any) {
    console.log(label, "err", err.name, err.code ?? "", err.message);
  }
}

const cbcKey = Buffer.alloc(16);
const cbcIv = Buffer.alloc(16);
const cipher = crypto.createCipheriv("aes-128-cbc", cbcKey, cbcIv);
cipher.update("abc");
report("cipher-final-1", () => cipher.final());
report("cipher-final-2", () => cipher.final());
report("cipher-update-after-final", () => cipher.update("x"));
report("cipher-set-auto-padding-after-final", () => cipher.setAutoPadding(false));
report("cipher-get-auth-tag-cbc", () => cipher.getAuthTag());

const cbcDecipher = crypto.createDecipheriv("aes-128-cbc", cbcKey, cbcIv);
report("decipher-set-auth-tag-cbc", () => cbcDecipher.setAuthTag(Buffer.alloc(16)));

const gcmCipher = crypto.createCipheriv("aes-128-gcm", cbcKey, Buffer.alloc(12));
console.log("gcm-cipher-methods:", typeof (gcmCipher as any).getAuthTag, typeof (gcmCipher as any).setAuthTag);
report("gcm-get-auth-tag-before-final", () => gcmCipher.getAuthTag());
gcmCipher.update("abc");
report("gcm-set-aad-after-update", () => gcmCipher.setAAD(Buffer.from("aad")));
report("gcm-final", () => gcmCipher.final());
report("gcm-get-auth-tag-after-final", () => gcmCipher.getAuthTag());

const gcmDecipher = crypto.createDecipheriv("aes-128-gcm", cbcKey, Buffer.alloc(12));
console.log("gcm-decipher-methods:", typeof (gcmDecipher as any).getAuthTag, typeof (gcmDecipher as any).setAuthTag);
report("decipher-final-no-tag", () => gcmDecipher.final());
report("decipher-set-auth-tag-after-final", () => gcmDecipher.setAuthTag(Buffer.alloc(16)));

const tagOnce = crypto.createDecipheriv("aes-128-gcm", cbcKey, Buffer.alloc(12));
report("decipher-set-auth-tag-first", () => tagOnce.setAuthTag(Buffer.alloc(16)));
report("decipher-set-auth-tag-second", () => tagOnce.setAuthTag(Buffer.alloc(16)));
