import * as crypto from "node:crypto";

const key = Buffer.alloc(32, 1);
const iv16 = Buffer.alloc(16, 2);
const iv12 = Buffer.alloc(12, 2);

// AES-256-CBC roundtrip
{
  const c = crypto.createCipheriv("aes-256-cbc", key, iv16);
  const enc = Buffer.concat([c.update(Buffer.from("hello-cbc")), c.final()]);
  const d = crypto.createDecipheriv("aes-256-cbc", key, iv16);
  const dec = Buffer.concat([d.update(enc), d.final()]);
  console.log("cbc:", dec.toString());
}

// AES-256-GCM roundtrip with auth tag
{
  const c = crypto.createCipheriv("aes-256-gcm", key, iv12);
  const enc = Buffer.concat([c.update(Buffer.from("hello-gcm")), c.final()]);
  const tag = c.getAuthTag();
  console.log("gcm tag length:", tag.length);
  const d = crypto.createDecipheriv("aes-256-gcm", key, iv12);
  d.setAuthTag(tag);
  const dec = Buffer.concat([d.update(enc), d.final()]);
  console.log("gcm:", dec.toString());
}
