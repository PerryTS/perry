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
  const limited = await crypto.subtle.generateKey(
    { name: "AES-GCM", length: 128 },
    false,
    ["encrypt"],
  );
  console.log("limited aes:", limited.type, limited.algorithm.name, limited.extractable, limited.usages.join(","));
  await logReject("limited export", crypto.subtle.exportKey("raw", limited));
  await logReject(
    "limited decrypt",
    crypto.subtle.decrypt({ name: "AES-GCM", iv: new Uint8Array(12) }, limited, new Uint8Array(16)),
  );
  await logReject(
    "aes empty usages",
    crypto.subtle.generateKey({ name: "AES-GCM", length: 128 }, true, []),
  );

  const imported = await crypto.subtle.importKey("raw", new Uint8Array(16), "AES-GCM", false, ["encrypt"]);
  console.log("imported aes:", imported.type, imported.algorithm.name, imported.extractable, imported.usages.join(","));
  await logReject("imported export", crypto.subtle.exportKey("raw", imported));
  await logReject("import aes empty usages", crypto.subtle.importKey("raw", new Uint8Array(16), "AES-GCM", true, []));

  const signOnly = await crypto.subtle.generateKey(
    { name: "HMAC", hash: "SHA-256", length: 256 },
    true,
    ["sign"],
  );
  const verifyOnly = await crypto.subtle.generateKey(
    { name: "HMAC", hash: "SHA-256", length: 256 },
    true,
    ["verify"],
  );
  const data = new TextEncoder().encode("usage check");
  const sig = await crypto.subtle.sign("HMAC", signOnly, data);
  console.log("hmac sign-only usages:", signOnly.usages.join(","));
  await logReject("hmac sign with verify-only", crypto.subtle.sign("HMAC", verifyOnly, data));
  await logReject("hmac verify with sign-only", crypto.subtle.verify("HMAC", signOnly, sig, data));
  await logReject(
    "import hmac bad usage",
    crypto.subtle.importKey(
      "raw",
      new Uint8Array(32),
      { name: "HMAC", hash: "SHA-256" },
      true,
      ["encrypt" as any],
    ),
  );
}

await main();
