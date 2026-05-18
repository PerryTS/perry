// Regression test for crypto.subtle.encrypt / decrypt (AES-GCM).
// jose's `gcmEncrypt` / `gcmDecrypt` exercise this exact shape.

async function main() {
  const keyBytes = new Uint8Array(16); // 128-bit key — zero-filled is fine for the round-trip
  for (let i = 0; i < keyBytes.length; i++) keyBytes[i] = i;
  const iv = new Uint8Array(12);
  for (let i = 0; i < iv.length; i++) iv[i] = i + 100;

  const key = await crypto.subtle.importKey("raw", keyBytes, "AES-GCM", false, ["encrypt", "decrypt"]);

  const plaintext = new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
  const ct = await crypto.subtle.encrypt({ name: "AES-GCM", iv: iv }, key, plaintext);
  const ctView = new Uint8Array(ct);
  console.log("ct.length =", ctView.length); // 10 plaintext + 16 tag

  const pt = await crypto.subtle.decrypt({ name: "AES-GCM", iv: iv }, key, ctView);
  const ptView = new Uint8Array(pt);
  console.log("pt.length =", ptView.length);
  let ok = ptView.length === plaintext.length;
  for (let i = 0; i < plaintext.length; i++) {
    if (ptView[i] !== plaintext[i]) ok = false;
  }
  console.log("round-trip ok =", ok);
}

main();
