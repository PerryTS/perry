import * as crypto from "node:crypto";

async function logReject(label: string, promise: Promise<unknown>) {
  let rejected = false;
  let name = "";
  try {
    await promise;
  } catch (e: any) {
    rejected = true;
    name = e?.name ?? "";
  }
  console.log(`${label}:`, rejected, name);
}

async function main() {
  const data = new TextEncoder().encode("generated hmac");
  const key = await crypto.subtle.generateKey(
    { name: "HMAC", hash: "SHA-256", length: 256 },
    true,
    ["sign", "verify"],
  );
  console.log(
    "hmac generated key:",
    key.type,
    key.algorithm.name,
    key.algorithm.hash.name,
    key.algorithm.length,
    key.extractable,
    key.usages.join(","),
  );
  const sig = await crypto.subtle.sign("HMAC", key, data);
  console.log("hmac generated sig:", sig.byteLength > 0);
  console.log("hmac generated verify:", await crypto.subtle.verify("HMAC", key, sig, data));

  await logReject(
    "hmac empty usages",
    crypto.subtle.generateKey({ name: "HMAC", hash: "SHA-256", length: 256 }, true, []),
  );
  await logReject(
    "hmac bad usage",
    crypto.subtle.generateKey({ name: "HMAC", hash: "SHA-256", length: 256 }, true, ["encrypt" as any]),
  );
}

await main();
