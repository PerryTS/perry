// #11516: createHash/createHmac objects that never escape are computed
// without registering a native handle. Every shape below must match node
// byte-for-byte, whether perry takes the handle-free path (inline chains,
// block-local objects used only by update/digest) or keeps the registered
// handle (objects that escape). Runtime-built data strings stay longer than
// 5 bytes (short-string encoding, #11481).
import * as crypto from "node:crypto";

function show(label: string, v: unknown): void {
  if (typeof v === "string") console.log(label, JSON.stringify(v));
  else if (v instanceof Uint8Array) console.log(label, "Buffer", Buffer.from(v).toString("hex"));
  else console.log(label, typeof v, String(v));
}

function attempt(label: string, f: () => unknown): void {
  try {
    show(label, f());
  } catch (e: any) {
    console.log(label, "threw", e.name, e.code, e.message);
  }
}

const path = "request-path-/api/items/" + String(7);
const key = "secret-key-" + String(42);
const buf = Buffer.from("buffer-input-bytes-0123456789", "utf8");
const u8 = new Uint8Array([1, 2, 3, 250, 251, 252, 253, 254, 255]);

// --- inline chains: every algorithm, 0..3 updates, every encoding --------
for (const alg of ["sha1", "sha224", "sha256", "sha384", "sha512", "md5", "SHA256", "sha512-256"]) {
  show("chain1 " + alg, crypto.createHash(alg).update(path).digest("hex"));
}
show("chain0", crypto.createHash("sha1").digest("hex"));
show("chain3", crypto.createHash("sha256").update(path).update(buf).update(u8).digest("base64"));
show("chain-inenc", crypto.createHash("sha256").update("68656c6c6f", "hex").update("aGVsbG8=", "base64").digest("base64url"));
show("chain-buffer", crypto.createHash("sha512").update(path).digest());
show("chain-latin1", crypto.createHash("md5").update(buf).digest("latin1"));
show("chain-binary", crypto.createHash("md5").update(buf).digest("binary"));
show("chain-utf8", crypto.createHash("sha1").update(path).digest("utf8"));
show("chain-ucs2", crypto.createHash("sha1").update(path).digest("ucs2"));
show("chain-ascii", crypto.createHash("sha1").update(path).digest("ascii"));
show("chain-HEX", crypto.createHash("sha1").update(path).digest("HEX"));
show("chain-unknown-enc", crypto.createHash("sha1").update(path).digest("nope"));
show("chain-buffer-enc", crypto.createHash("sha1").update(path).digest("buffer"));
show("chain-md5-noenc", crypto.createHash("md5").update(path).digest());
show("chain-shake", crypto.createHash("shake256", { outputLength: 7 }).update(path).digest("hex"));
show("chain-literal", crypto.createHash("sha256").update("abc").digest("hex"));
show("hmac1", crypto.createHmac("sha256", key).update(path).digest("hex"));
show("hmac-buf-key", crypto.createHmac("sha512", buf).update(path).update(u8).digest("base64"));
show("hmac-noenc", crypto.createHmac("sha1", key).update(path).digest());
show("hmac-md5", crypto.createHmac("md5", key).update(path).digest("hex"));
show("hash-oneshot", crypto.hash("sha1", path));
const enc = "base64";
show("chain-var-enc", crypto.createHash("sha256").update(path).digest(enc));
const digestLen = crypto.createHash("sha256").update(path).digest("hex").length;
console.log("chain-length", digestLen);

// --- block-local objects that never escape ------------------------------
function localHash(data: string): string {
  const h = crypto.createHash("sha1");
  h.update(data);
  return h.digest("hex");
}
show("local", localHash(path));

function localBranches(data: string, extra: boolean): string {
  const h = crypto.createHash("sha256").update("prefix-bytes");
  if (extra) {
    h.update(data);
  }
  for (let i = 0; i < 3; i++) h.update(buf).update(u8);
  return h.digest("base64");
}
show("local-branch-a", localBranches(path, true));
show("local-branch-b", localBranches(path, false));

// jsonwebtoken/jwa shape: `(hmac.update(x), hmac.digest(enc))`.
function jwaSign(thing: string, secret: string): string {
  var hmac = crypto.createHmac("sha" + 256, secret);
  var sig = (hmac.update(thing), hmac.digest("base64"));
  return sig;
}
show("jwa", jwaSign(path, key));

let total = 0;
for (let i = 0; i < 50000; i++) {
  const h = crypto.createHash("sha1");
  h.update("request-path-/api/items/" + (i & 7));
  total += h.digest("hex").length;
}
console.log("loop-local", total);
let chainTotal = 0;
for (let i = 0; i < 50000; i++) {
  chainTotal += crypto.createHmac("sha256", key).update("payload-" + i).digest("hex").length;
}
console.log("loop-chain", chainTotal);

// Finalized state is still observable on a non-escaping local.
attempt("local-double-digest", () => {
  const h = crypto.createHash("sha1");
  h.update(path);
  h.digest("hex");
  return h.digest("hex");
});
attempt("local-update-after-digest", () => {
  const h = crypto.createHash("md5");
  h.digest();
  h.update(path);
  return "unreachable";
});
attempt("hmac-double-digest", () => {
  const m = crypto.createHmac("sha256", key);
  m.update(path);
  m.digest("hex");
  return m.digest("hex");
});

// --- escaping objects keep the registered handle -------------------------
const kept: crypto.Hash[] = [];
function stored(): string {
  const h = crypto.createHash("sha1");
  kept.push(h);
  h.update(path);
  return h.digest("hex");
}
show("escape-stored", stored());
function consume(h: crypto.Hash): string {
  return h.update(path).digest("hex");
}
function passed(): string {
  const h = crypto.createHash("sha256");
  return consume(h);
}
show("escape-passed", passed());
function returned(): crypto.Hash {
  const h = crypto.createHash("sha384");
  h.update(path);
  return h;
}
show("escape-returned", returned().digest("hex"));
function copied(): string {
  const h = crypto.createHash("sha256");
  h.update(path);
  const c = h.copy();
  c.update("more-bytes");
  return h.digest("hex") + " " + c.digest("hex");
}
show("escape-copy", copied());
function captured(): () => string {
  const h = crypto.createHash("md5");
  h.update(path);
  return () => h.digest("hex");
}
show("escape-captured", captured()());
const aliasSource = crypto.createHash("sha1");
const alias = aliasSource.update(path);
show("escape-alias", alias.digest("hex") + " " + String(alias === aliasSource));

async function awaited(): Promise<string> {
  const h = crypto.createHash("sha1");
  h.update(path);
  await Promise.resolve();
  h.update(buf);
  return h.digest("hex");
}

// --- errors ---------------------------------------------------------------
attempt("err-alg", () => crypto.createHash("nope").update(path).digest("hex"));
attempt("err-alg-local", () => {
  const h = crypto.createHash("no-such-digest");
  h.update(path);
  return h.digest("hex");
});
attempt("err-hmac-alg", () => crypto.createHmac("nope", key).update(path).digest("hex"));
attempt("err-data", () => crypto.createHash("sha256").update(123 as any).digest("hex"));
attempt("err-data-local", () => {
  const h = crypto.createHash("sha256");
  h.update(null as any);
  return h.digest("hex");
});
attempt("err-alg-type", () => crypto.createHash(5 as any).update(path).digest("hex"));
attempt("err-key-type", () => crypto.createHmac("sha256", 5 as any).update(path).digest("hex"));

awaited().then((v) => {
  show("escape-await", v);
  // Streams: the object is used as a Transform, so it is not a chain.
  const s = crypto.createHash("sha1");
  s.on("data", (d: Buffer) => show("stream-data", d));
  s.on("end", () => console.log("stream-end"));
  s.write(path);
  s.end();
});
