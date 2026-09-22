// #10472 — Sign.sign must encode string output and Verify.verify must decode
// string signatures with the optional encoding argument.
import crypto from "node:crypto";

const privateKey =
  "-----BEGIN PRIVATE KEY-----\n" +
  "MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgzXENgrYISpXh8UGG\n" +
  "n6gRdTvn03fTF16DgDQTADm0XV6hRANCAAS86N32ME7tVKj5oIMLOiYoElFNSXbJ\n" +
  "wMQL3GyWDLKC996gWUP4WfQLYOJd6To9wdlomuiOFtVryzwKdMdFFd7G\n" +
  "-----END PRIVATE KEY-----\n";
const publicKey =
  "-----BEGIN PUBLIC KEY-----\n" +
  "MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEvOjd9jBO7VSo+aCDCzomKBJRTUl2\n" +
  "ycDEC9xslgyygvfeoFlD+Fn0C2DiXek6PcHZaJrojhbVa8s8CnTHRRXexg==\n" +
  "-----END PUBLIC KEY-----\n";
const rsaPrivateKey =
  "-----BEGIN PRIVATE KEY-----\n" +
  "MIICdwIBADANBgkqhkiG9w0BAQEFAASCAmEwggJdAgEAAoGBALA9qACs400Jgizt\n" +
  "8uNV2sw/+Qj1V6/27b50gH4LBC81YPypipBTZ52mbb4Xfpr5OroUnWCxaibEj0rg\n" +
  "2nlKuS6wSCOrEdsdRC40GoLeSnIDExDgYVTWlEiU2dZ2bAKqSO3l7JRBQyJkwfBG\n" +
  "qJRHTtrZ3ycKmQTvlcZJw+p5r48fAgMBAAECgYA7hxfP4pWD18pYUqbPkpgslQ8Q\n" +
  "r43Gqajzw3YDHMV1DJqNvNZImWNOJIC8zEK/JZ9oar4dgs9P+ORNblVc0phpVSQ4\n" +
  "lKjOQguiFgZqjbEL1tQTpObQmf711ZcWOMiFweDKbT0foW1b+0BnzLVLQHsrnItv\n" +
  "obASCIEv9vKytN1wwQJBANq7Q8811TaV5xrlzvITBIZxO/g8oneaHRfxLLGNBuWh\n" +
  "TLPkzkJh2higWM0nKk6lcyKwCzAqI5DKMkaXpvmoBoMCQQDORQN37EXMed080eLI\n" +
  "RwuyG+2ZGoxtyQtUyoznlIXHWsoE4uUBmQ4YjCNljhbqPz0RTvrDxPj0uzJnP2Vd\n" +
  "+xI1AkEAzjuC0/yN68mq/VFwrg4AVkKtqICDLwHALLLY0Q+HUTukdnllgHGCkXWe\n" +
  "RNCIs16MEEisQ913ay05+hVC+mHSwQJBAKl+4mu/9lcg6KBao+0JHF4+Ps6ply17\n" +
  "n9kMHB8L16ZKP2kmfSIEACZBubBwwvm3/1liugL2r9CCptdaq9Q/ROUCQBWhzBBS\n" +
  "LbtQCOQOxrzUW6ipqfEeoHWEEZI++krTqYFsZL62uZ86b6gLyQLLRSsngr7D4D/w\n" +
  "+89KirPwQz+VOmA=\n" +
  "-----END PRIVATE KEY-----\n";
const rsaPublicKey =
  "-----BEGIN PUBLIC KEY-----\n" +
  "MIGfMA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQCwPagArONNCYIs7fLjVdrMP/kI\n" +
  "9Vev9u2+dIB+CwQvNWD8qYqQU2edpm2+F36a+Tq6FJ1gsWomxI9K4Np5SrkusEgj\n" +
  "qxHbHUQuNBqC3kpyAxMQ4GFU1pRIlNnWdmwCqkjt5eyUQUMiZMHwRqiUR07a2d8n\n" +
  "CpkE75XGScPqea+PHwIDAQAB\n" +
  "-----END PUBLIC KEY-----\n";
const payload = "header.payload";

function sign(key: any, encoding?: BufferEncoding): Buffer | string {
  const signer = crypto.createSign("sha256");
  signer.update(payload);
  return encoding === undefined ? signer.sign(key) : signer.sign(key, encoding);
}

function verify(
  key: any,
  signature: Buffer | string,
  encoding?: BufferEncoding,
): boolean {
  const verifier = crypto.createVerify("sha256");
  verifier.update(payload);
  return encoding === undefined
    ? verifier.verify(key, signature)
    : verifier.verify(key, signature, encoding);
}

for (const encoding of ["hex", "base64", "base64url", "latin1", "binary"] as const) {
  const signature = sign(privateKey, encoding);
  console.log(
    "encoded",
    encoding,
    typeof signature,
    Buffer.isBuffer(signature),
    verify(publicKey, signature, encoding),
  );
}

const p1363Private = { key: privateKey, dsaEncoding: "ieee-p1363" as const };
const p1363Public = { key: publicKey, dsaEncoding: "ieee-p1363" as const };
const p1363 = sign(p1363Private, "base64url");
console.log(
  "p1363",
  typeof p1363,
  Buffer.isBuffer(p1363),
  verify(p1363Public, p1363, "base64url"),
);

const rsa = sign(rsaPrivateKey, "base64");
console.log(
  "rsa",
  typeof rsa,
  Buffer.isBuffer(rsa),
  verify(rsaPublicKey, rsa, "base64"),
);

const rsaRaw = sign(rsaPrivateKey) as Buffer;
for (const encoding of ["utf8", "ascii", "utf16le"] as const) {
  const encoded = sign(rsaPrivateKey, encoding);
  console.log(
    "encoded-lossy",
    encoding,
    typeof encoded,
    Buffer.isBuffer(encoded),
    encoded === rsaRaw.toString(encoding),
  );
}

const rsaPssPrivate = {
  key: rsaPrivateKey,
  padding: crypto.constants.RSA_PKCS1_PSS_PADDING,
  saltLength: 32,
};
const rsaPssPublic = {
  key: rsaPublicKey,
  padding: crypto.constants.RSA_PKCS1_PSS_PADDING,
  saltLength: 32,
};
const rsaPss = sign(rsaPssPrivate, "hex");
console.log(
  "rsa-pss",
  typeof rsaPss,
  Buffer.isBuffer(rsaPss),
  verify(rsaPssPublic, rsaPss, "hex"),
);

const raw = sign(privateKey) as Buffer;
console.log(
  "buffer-invalid-encoding-ignored",
  verify(publicKey, raw, "not-an-encoding" as any),
);

try {
  sign(privateKey, "not-an-encoding" as any);
  console.log("sign-invalid no-throw");
} catch (error: any) {
  console.log("sign-invalid", error.code);
}

try {
  verify(
    publicKey,
    (sign(privateKey, "hex") as string),
    "not-an-encoding" as any,
  );
  console.log("verify-invalid no-throw");
} catch (error: any) {
  console.log("verify-invalid", error.code);
}
