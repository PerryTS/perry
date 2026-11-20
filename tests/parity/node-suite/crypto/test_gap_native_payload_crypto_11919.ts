// #11919 P0: crypto Hash / Hmac / Cipheriv / Decipheriv are ordinary objects
// that own a native payload. Shape, identity, instanceof, collection keys,
// finalized-state errors and a churn loop, all against node.
import * as crypto from "crypto";

function shape(label: string, o: any): void {
  console.log(
    label,
    typeof o,
    JSON.stringify(Object.keys(o)),
    JSON.stringify(o),
    Object.prototype.toString.call(o),
    o.constructor.name,
  );
}

const h1 = crypto.createHash("sha256");
const h2 = crypto.createHash("sha256");
const m1 = crypto.createHmac("sha256", "key");
const c1 = crypto.createCipheriv("aes-128-cbc", Buffer.alloc(16, 1), Buffer.alloc(16, 2));
const d1 = crypto.createDecipheriv("aes-128-cbc", Buffer.alloc(16, 1), Buffer.alloc(16, 2));
shape("hash", h1);
shape("hmac", m1);
shape("cipher", c1);
shape("decipher", d1);
shape("hash-opts", crypto.createHash("shake256", { outputLength: 8 }));

console.log("identity", h1 === h2, h1 !== h2, h1 === h1);
console.log(
  "instanceof",
  h1 instanceof crypto.Hash,
  m1 instanceof crypto.Hmac,
  c1 instanceof crypto.Cipheriv,
  d1 instanceof crypto.Decipheriv,
  h1 instanceof crypto.Hmac,
  c1 instanceof crypto.Decipheriv,
);
console.log("constructor", h1.constructor === crypto.Hash, Object.getPrototypeOf(h1) === Object.getPrototypeOf(h2));
console.log("methods", typeof h1.update, typeof h1.digest, typeof h1.copy, typeof c1.getAuthTag, typeof d1.setAuthTag);

const map = new Map<any, string>();
map.set(h1, "first");
map.set(h2, "second");
const weak = new WeakMap<object, number>();
weak.set(h1, 1);
weak.set(m1, 2);
const set = new Set<any>([h1, h2, h1]);
console.log("keys", map.get(h1), map.get(h2), map.size, weak.get(h1), weak.get(m1), weak.has(h2), set.size);

// Own data properties behave like any object's.
(h1 as any).tag = "mine";
console.log("expando", (h1 as any).tag, JSON.stringify(Object.keys(h1)));

console.log("chain", h1.update("a") === h1, h1.update("b").digest("hex"));
try {
  h1.update("c");
} catch (e: any) {
  console.log("after-digest update", e.code, e.message);
}
try {
  h1.digest("hex");
} catch (e: any) {
  console.log("after-digest digest", e.code);
}
try {
  h1.copy();
} catch (e: any) {
  console.log("after-digest copy", e.code);
}
console.log("still an object", JSON.stringify(Object.keys(h1)), map.get(h1));

const base = crypto.createHash("sha1").update("prefix");
const copy = base.copy();
console.log("copy", copy !== base, base.digest("hex"), copy.update("-more").digest("hex"));

console.log("hmac", m1.update("data").digest("base64"), JSON.stringify(m1.digest("hex")));

const enc = Buffer.concat([c1.update(Buffer.from("payload pattern")), c1.final()]);
const dec = Buffer.concat([d1.update(enc), d1.final()]);
console.log("cipher", enc.toString("hex"), dec.toString());
try {
  c1.final();
} catch (e: any) {
  console.log("cipher final twice", e.code);
}

const key = Buffer.alloc(32, 7);
const iv = Buffer.alloc(12, 9);
const g = crypto.createCipheriv("aes-256-gcm", key, iv);
const gct = Buffer.concat([g.update("authenticated"), g.final()]);
const tag = g.getAuthTag();
const gd = crypto.createDecipheriv("aes-256-gcm", key, iv);
gd.setAuthTag(tag);
console.log("gcm", gct.toString("hex"), tag.length, Buffer.concat([gd.update(gct), gd.final()]).toString());

// Churn: every object is dropped by digest()/final() or by the collector.
let acc = 0;
const kept: any[] = [];
for (let i = 0; i < 20000; i++) {
  const h = crypto.createHash("md5");
  h.update("n" + i);
  if (i % 1000 === 0) kept.push(h);
  else acc += h.digest()[0];
  const u = crypto.createHmac("sha1", "k");
  if (i % 2 === 0) acc += u.update("x").digest()[1];
}
console.log("churn", acc, kept.length, kept[3].digest("hex"));
