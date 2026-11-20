// Issue #561 acceptance — `signing.ts:317-330` shape from
// `@bradenmacdonald/s3-lite-client`. Drives the full AWS SigV4 derived-
// key chain (kSecret → kDate → kRegion → kService → kSigning) through
// `crypto.subtle.{importKey,sign}` and prints the final hex string.
// Compared byte-for-byte against `node --experimental-strip-types`.

declare const crypto: any;

function bytesToHex(bytes: Uint8Array): string {
  let s = "";
  for (let i = 0; i < bytes.length; i++) {
    const h = bytes[i].toString(16);
    s += h.length === 1 ? "0" + h : h;
  }
  return s;
}

async function sha256hmac(key: Uint8Array, msg: string): Promise<Uint8Array> {
  // Exactly the shape s3-lite-client uses internally (issue's repro
  // points at signing.ts:317-330).
  const k = await crypto.subtle.importKey(
    "raw",
    key,
    { name: "HMAC", hash: { name: "SHA-256" } },
    false,
    ["sign", "verify"],
  );
  const enc = new TextEncoder();
  const sig = await crypto.subtle.sign("HMAC", k, enc.encode(msg));
  return new Uint8Array(sig);
}

async function main() {
  // The canonical AWS SigV4 derived-signing-key example from
  // https://docs.aws.amazon.com/general/latest/gr/sigv4-signed-request-examples.html
  const enc = new TextEncoder();
  const secret = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";
  const kSecret = enc.encode("AWS4" + secret);
  const kDate = await sha256hmac(kSecret, "20150830");
  const kRegion = await sha256hmac(kDate, "us-east-1");
  const kService = await sha256hmac(kRegion, "iam");
  const kSigning = await sha256hmac(kService, "aws4_request");
  console.log(bytesToHex(kSigning));
  // c4afb1cc5771d871763a393e44b703571b55cc28424d1a5e86da6ed3c154a4b9
}

main();
