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
  const kek = await crypto.subtle.generateKey(
    { name: "AES-KW", length: 256 },
    true,
    ["wrapKey", "unwrapKey"],
  );
  const target = await crypto.subtle.generateKey(
    { name: "AES-GCM", length: 128 },
    true,
    ["encrypt", "decrypt"],
  );
  const wrapped = await crypto.subtle.wrapKey("jwk", target, kek, "AES-KW");
  console.log("wrap jwk:", wrapped.byteLength > 0);
  const unwrapped = await crypto.subtle.unwrapKey(
    "jwk",
    wrapped,
    kek,
    "AES-KW",
    { name: "AES-GCM" },
    false,
    ["encrypt"],
  );
  console.log("unwrap jwk:", unwrapped.type, unwrapped.algorithm.name, unwrapped.extractable, unwrapped.usages.join(","));
  await logReject("unwrap jwk export", crypto.subtle.exportKey("raw", unwrapped));

  const wrapOnly = await crypto.subtle.generateKey({ name: "AES-KW", length: 256 }, true, ["wrapKey"]);
  const unwrapOnly = await crypto.subtle.generateKey({ name: "AES-KW", length: 256 }, true, ["unwrapKey"]);
  await logReject("wrap with unwrap-only kek", crypto.subtle.wrapKey("raw", target, unwrapOnly, "AES-KW"));
  await logReject(
    "unwrap with wrap-only kek",
    crypto.subtle.unwrapKey("raw", await crypto.subtle.wrapKey("raw", target, wrapOnly, "AES-KW"), wrapOnly, "AES-KW", { name: "AES-GCM" }, true, ["encrypt"]),
  );
  const nonExtractableTarget = await crypto.subtle.generateKey({ name: "AES-GCM", length: 128 }, false, ["encrypt"]);
  await logReject("wrap nonextractable target", crypto.subtle.wrapKey("raw", nonExtractableTarget, wrapOnly, "AES-KW"));
  await logReject("wrap spki aes", crypto.subtle.wrapKey("spki", target, kek, "AES-KW"));
}

await main();
