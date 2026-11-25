import { Buffer } from "node:buffer";

const F4 = new Uint8Array([1, 0, 1]);

function bitLength(n: string): number {
  return BigInt("0x" + Buffer.from(n, "base64url").toString("hex")).toString(2).length;
}

async function attempt(label: string, algorithm: any, usages: any[] = ["sign", "verify"]) {
  try {
    const pair: any = await crypto.subtle.generateKey(algorithm, true, usages);
    const jwk: any = await crypto.subtle.exportKey("jwk", pair.publicKey);
    const privateJwk: any = await crypto.subtle.exportKey("jwk", pair.privateKey);
    console.log(
      label,
      bitLength(jwk.n),
      jwk.e,
      jwk.n === privateJwk.n,
      pair.publicKey.algorithm.modulusLength,
      Array.from(pair.privateKey.algorithm.publicExponent as Uint8Array).join(),
    );
    return pair;
  } catch (error: any) {
    console.log(label, error instanceof TypeError, error instanceof DOMException, error.name, error.code, error.message);
  }
}

const pss = { name: "RSA-PSS", hash: "SHA-256" };

for (const [name, usages] of [
  ["RSASSA-PKCS1-v1_5", ["sign", "verify"]],
  ["RSA-PSS", ["sign", "verify"]],
  ["RSA-OAEP", ["encrypt", "decrypt"]],
] as const) {
  const base = { name, hash: "SHA-256" };
  await attempt(`${name} 3072`, { ...base, modulusLength: 3072, publicExponent: F4 }, [...usages]);
  await attempt(`${name} e=3`, { ...base, modulusLength: 1024, publicExponent: new Uint8Array([3]) }, [...usages]);
  await attempt(`${name} small`, { ...base, modulusLength: 8, publicExponent: F4 }, [...usages]);
  await attempt(`${name} even`, { ...base, modulusLength: 1024, publicExponent: new Uint8Array([4]) }, [...usages]);
}

const data = Buffer.from("rsa keygen");
for (const modulusLength of [1024, 3072]) {
  const pair = await attempt(`sign ${modulusLength}`, { ...pss, modulusLength, publicExponent: F4 });
  const signature = await crypto.subtle.sign({ name: "RSA-PSS", saltLength: 32 }, pair.privateKey, data);
  console.log(signature.byteLength, await crypto.subtle.verify({ name: "RSA-PSS", saltLength: 32 }, pair.publicKey, signature, data));
}

const lengths: any[] = [
  undefined, null, "1024", "x", NaN, Infinity, 1024.9, -1, 4294967296, 4294967296 + 1024, 1024n, Symbol("s"), {}, true,
  { valueOf: () => 1024 }, 0, 8, 511, 512, 1023, 2049, 1073741826, 2147483648,
];
for (const modulusLength of lengths) {
  await attempt(`modulusLength ${String(modulusLength)}`, { ...pss, modulusLength, publicExponent: F4 });
}
await attempt("modulusLength throws", { ...pss, publicExponent: F4, modulusLength: { valueOf() { throw new RangeError("from valueOf"); } } });
await attempt("modulusLength missing", { ...pss, publicExponent: F4 });

const exponents: any[] = [
  undefined, null, 65537, "x", [1, 0, 1], new ArrayBuffer(3), new Uint16Array([1, 0, 1]), new DataView(F4.buffer),
  new Uint8Array(0), new Uint8Array([0]), new Uint8Array([0, 0]), new Uint8Array([1]), new Uint8Array([2]), new Uint8Array([4]),
  new Uint8Array([1, 0, 1, 0]), new Uint8Array([1, 0, 0, 0, 0]), new Uint8Array([1, 0, 0, 0, 1]), new Uint8Array(33).fill(255),
  new Uint8Array([3]), new Uint8Array([0, 3]), new Uint8Array([0, 0, 0, 0, 0, 0, 0, 1, 0, 1]), new Uint8Array([255, 255, 255, 255]),
  Buffer.from([1, 0, 1]), new Uint8Array([9, 1, 0, 1, 9]).subarray(1, 4),
];
for (const publicExponent of exponents) {
  const kind = publicExponent?.constructor?.name ?? String(publicExponent);
  const bytes = ArrayBuffer.isView(publicExponent) ? Buffer.from(publicExponent.buffer as any, publicExponent.byteOffset, publicExponent.byteLength).toString("hex") : "";
  await attempt(`publicExponent ${kind} ${bytes}`, { ...pss, modulusLength: 1024, publicExponent });
}
await attempt("publicExponent missing", { ...pss, modulusLength: 1024 });
await attempt("both missing", pss);
await attempt("string algorithm", "RSA-PSS");
await attempt("size before exponent", { ...pss, modulusLength: 8, publicExponent: new Uint8Array([4]) });
await attempt("conversion before usages", { ...pss, modulusLength: "x", publicExponent: F4 }, ["encrypt"]);

const order: string[] = [];
await attempt("read order", {
  name: "RSA-PSS",
  hash: "SHA-256",
  get publicExponent() { order.push("publicExponent"); return F4; },
  get modulusLength() { order.push("modulusLength"); return 8; },
});
console.log(order.join());
