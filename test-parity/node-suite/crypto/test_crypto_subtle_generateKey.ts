// Regression test for #955 follow-up — `crypto.subtle.generateKey`
// produces a fresh AES-GCM CryptoKey that round-trips through
// subtle.encrypt + subtle.decrypt. jose's `generateSecret('A256GCM')`
// path reaches this; pre-fix the chain bailed at HIR lowering.

async function main() {
  // Generate a fresh 256-bit AES-GCM key.
  const key = await crypto.subtle.generateKey(
    { name: "AES-GCM", length: 256 },
    true,
    ["encrypt", "decrypt"],
  );

  // Round-trip: encrypt some plaintext + decrypt and confirm equality.
  const iv = new Uint8Array(12);
  crypto.getRandomValues(iv);
  const plaintext = new TextEncoder().encode("hello aes-gcm");

  const ct = await crypto.subtle.encrypt(
    { name: "AES-GCM", iv },
    key,
    plaintext,
  );
  const ctView = new Uint8Array(ct);
  // ciphertext || tag: tag is 16 bytes.
  if (ctView.length !== plaintext.length + 16) {
    console.log("FAIL: unexpected ciphertext length", ctView.length);
    return;
  }

  const pt = await crypto.subtle.decrypt(
    { name: "AES-GCM", iv },
    key,
    ct,
  );
  const ptView = new Uint8Array(pt);
  const decoded = new TextDecoder().decode(ptView);

  console.log(decoded);
  console.log(decoded === "hello aes-gcm" ? "OK" : "FAIL");
}

main();
