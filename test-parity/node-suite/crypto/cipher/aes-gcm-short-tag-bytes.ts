// Verify AES-GCM produces byte-identical auth tags to Node for the
// non-standard short tag lengths {4, 8}. The original concern in #1433
// was that Perry's RustCrypto Aes*Gcm backend returned wrong bytes for
// these (the issue text speculated GHASH-with-shorter-tag-len would
// differ from full-tag truncation). In practice Node's OpenSSL backend
// just truncates the 16-byte tag, and Perry's `aes-gcm` crate likewise
// truncates after computing the full 16-byte tag, so the bytes match.
//
// This test pins the invariant on the **encrypt** side; the matching
// **decrypt** path with sub-12-byte tags is a separate bug — the
// `aes-gcm` crate's `SealedTagSize` trait is `private` and only
// permits U12..=U16, so we can't instantiate `AesGcm<.., U4>` for
// decryption. Tracked as a follow-up.
import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const key = Buffer.alloc(32, 1);
const iv = Buffer.alloc(12, 2);
const data = Buffer.from("hello world");

for (const tagLen of [4, 8, 12, 16]) {
  const cipher = crypto.createCipheriv("aes-256-gcm", key, iv, { authTagLength: tagLen });
  const ct = Buffer.concat([cipher.update(data), cipher.final()]);
  const tag = cipher.getAuthTag();
  console.log(`tagLen=${tagLen} ct=${ct.toString("hex")} tag=${tag.toString("hex")}`);
}

// Round-trip with the supported {12, 16} tag lengths confirms the
// decrypt path is byte-correct in the supported range.
for (const tagLen of [12, 16]) {
  const c = crypto.createCipheriv("aes-256-gcm", key, iv, { authTagLength: tagLen });
  const ct = Buffer.concat([c.update(data), c.final()]);
  const tag = c.getAuthTag();
  const d = crypto.createDecipheriv("aes-256-gcm", key, iv, { authTagLength: tagLen });
  d.setAuthTag(tag);
  const pt = Buffer.concat([d.update(ct), d.final()]);
  console.log(`roundtrip-${tagLen}:`, pt.toString());
}
