// Regression test for `crypto.subtle.wrapKey` / `unwrapKey` —
// generate two AES-GCM CryptoKeys, wrap the first with the second
// (AES-GCM wrap, since AES-KW is also supported but jose uses
// AES-GCM under the hood for A256GCMKW), unwrap, then confirm the
// recovered key decrypts a previously-encrypted ciphertext.

async function main() {
  // 1) Generate the "real" key (the one we'll wrap).
  const innerKey = await crypto.subtle.generateKey(
    { name: "AES-GCM", length: 256 },
    true,
    ["encrypt", "decrypt"],
  );

  // 2) Generate the wrapping/KEK key.
  const kek = await crypto.subtle.generateKey(
    { name: "AES-GCM", length: 256 },
    true,
    ["wrapKey", "unwrapKey"],
  );

  // 3) Encrypt a plaintext with the inner key — we'll decrypt with
  // the recovered key to confirm round-tripping preserved the bytes.
  const iv = new Uint8Array(12);
  crypto.getRandomValues(iv);
  const plaintext = new TextEncoder().encode("hello wrap/unwrap");
  const ct = await crypto.subtle.encrypt(
    { name: "AES-GCM", iv },
    innerKey,
    plaintext,
  );

  // 4) Wrap the inner key with the KEK (AES-GCM wrap).
  const wrapIv = new Uint8Array(12);
  crypto.getRandomValues(wrapIv);
  const wrapped = await crypto.subtle.wrapKey(
    "raw",
    innerKey,
    kek,
    { name: "AES-GCM", iv: wrapIv },
  );

  // 5) Unwrap to recover an AES-GCM CryptoKey.
  const recovered = await crypto.subtle.unwrapKey(
    "raw",
    wrapped,
    kek,
    { name: "AES-GCM", iv: wrapIv },
    { name: "AES-GCM", length: 256 },
    true,
    ["encrypt", "decrypt"],
  );

  // 6) Decrypt the original ciphertext with the recovered key.
  const pt = await crypto.subtle.decrypt(
    { name: "AES-GCM", iv },
    recovered,
    ct,
  );
  const decoded = new TextDecoder().decode(new Uint8Array(pt));

  console.log(decoded);
  console.log(decoded === "hello wrap/unwrap" ? "OK" : "FAIL");
}

main();
