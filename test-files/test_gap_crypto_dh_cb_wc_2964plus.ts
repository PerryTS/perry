import crypto from "node:crypto";

// #2964 — DiffieHellman / ECDH key getters throw before keys exist.
function probe(label: string, fn: () => void) {
  try {
    fn();
    console.log(label + " NOTHROW");
  } catch (e: any) {
    console.log(label + " throw " + e.name + " " + e.code);
  }
}
const dh = crypto.createDiffieHellman(512);
probe("dh.getPublicKey before", () => dh.getPublicKey());
probe("dh.getPrivateKey before", () => dh.getPrivateKey());
const ecdh = crypto.createECDH("prime256v1");
probe("ecdh.getPublicKey before", () => ecdh.getPublicKey());
probe("ecdh.getPrivateKey before", () => ecdh.getPrivateKey());

// #2932 — WebCrypto byte results resolve as ArrayBuffer, not a typed-array
// view. Sequenced before the callback test so output ordering is
// deterministic (the two are otherwise independent async operations).
(async () => {
  const data = new Uint8Array([1, 2, 3]);
  const digest = await crypto.subtle.digest("SHA-256", data);
  console.log(
    "digest isAB=" +
      (digest instanceof ArrayBuffer) +
      " isView=" +
      ArrayBuffer.isView(digest) +
      " len=" +
      digest.byteLength,
  );

  // #2955 — callback-form crypto APIs fire asynchronously (on a later tick),
  // after the synchronous "after" code runs. The flag flips to "after-call"
  // before the callback observes it.
  let flag = "init";
  crypto.randomBytes(4, (err: any, buf: any) => {
    console.log(
      "randomBytes cb flag=" +
        flag +
        " isBuf=" +
        Buffer.isBuffer(buf) +
        " len=" +
        (buf ? buf.length : -1),
    );
  });
  flag = "after-call";
  console.log("randomBytes sync-return flag=" + flag);
})();
