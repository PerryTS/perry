// Issue #561 — `crypto.subtle.{digest,importKey,sign,verify}` for AWS
// SigV4 / JWT / web-push signing chains. Compared byte-for-byte against
// `node --experimental-strip-types`.

declare const crypto: any;

function bytesToHex(bytes: Uint8Array): string {
  let s = "";
  for (let i = 0; i < bytes.length; i++) {
    const h = bytes[i].toString(16);
    s += h.length === 1 ? "0" + h : h;
  }
  return s;
}

async function main() {
  // ── digest ──────────────────────────────────────────────────────────
  // SHA-256 of "abc" — NIST FIPS 180-2 test vector.
  const enc = new TextEncoder();
  const abcDigest = await crypto.subtle.digest("SHA-256", enc.encode("abc"));
  const abcHex = bytesToHex(new Uint8Array(abcDigest));
  console.log(abcHex);
  // ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad

  // SHA-256 of empty string.
  const emptyDigest = await crypto.subtle.digest("SHA-256", new Uint8Array(0));
  console.log(bytesToHex(new Uint8Array(emptyDigest)));
  // e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855

  // ── importKey + sign — AWS SigV4 first chain step ─────────────────
  // From https://docs.aws.amazon.com/general/latest/gr/sigv4_signing.html:
  //   kSecret = "AWS4" + secret
  //   kDate = HMAC-SHA-256(kSecret, "20150830")
  // Expected hex:
  //   0138c7a6cbd60aa727b2f653a522567439dfb9f3e72b21f9b25941a42f04a7cd
  // (computed independently against Node's WebCrypto.)
  const secret = "AWS4wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";
  const keyBytes = enc.encode(secret);
  const kSecret = await crypto.subtle.importKey(
    "raw",
    keyBytes,
    { name: "HMAC", hash: { name: "SHA-256" } },
    false,
    ["sign", "verify"],
  );

  const date = enc.encode("20150830");
  const sigBuf = await crypto.subtle.sign("HMAC", kSecret, date);
  const sigHex = bytesToHex(new Uint8Array(sigBuf));
  console.log(sigHex);

  // ── verify ─────────────────────────────────────────────────────────
  // Round-trip the signature back through verify — it should match.
  const ok = await crypto.subtle.verify(
    "HMAC",
    kSecret,
    new Uint8Array(sigBuf),
    date,
  );
  console.log(ok); // true

  // Tampered signature must NOT verify.
  const tampered = new Uint8Array(sigBuf);
  tampered[0] = tampered[0] ^ 0xff;
  const bad = await crypto.subtle.verify("HMAC", kSecret, tampered, date);
  console.log(bad); // false

  // ── Algorithm-as-object form (RFC4231 Test Case 2 — HMAC-SHA-256) ──
  // key = "Jefe", data = "what do ya want for nothing?"
  // expected = 5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843
  const jefe = await crypto.subtle.importKey(
    "raw",
    enc.encode("Jefe"),
    { name: "HMAC", hash: { name: "SHA-256" } },
    false,
    ["sign"],
  );
  const tc2 = await crypto.subtle.sign(
    { name: "HMAC" },
    jefe,
    enc.encode("what do ya want for nothing?"),
  );
  console.log(bytesToHex(new Uint8Array(tc2)));
  // 5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843
}

main();
