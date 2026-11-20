import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const jwkKey = {
  kty: "EC",
  crv: "P-256",
  x: "UachlYxCg48kyuIpXA7RRci2bb99E7izkzDQfX1sc6U",
  y: "umhCJJQF5niKkNIvna0egspwqEPc0XiuJ0vmpMOKdSg",
  d: "g_AptXAXWjIrPcyXQWW16JZdSV65Np7DOQxTl-SNhDQ",
};

const publicJwk = { kty: jwkKey.kty, crv: jwkKey.crv, x: jwkKey.x, y: jwkKey.y };
const data = Buffer.from("ec jwk dsa encoding parity");

const directP1363 = crypto.sign("sha256", data, { key: jwkKey, format: "jwk", dsaEncoding: "ieee-p1363" });
const directDer = crypto.sign("sha256", data, { key: jwkKey, format: "jwk", dsaEncoding: "der" });
const privateKey = crypto.createPrivateKey({ key: jwkKey, format: "jwk" });
const publicKey = crypto.createPublicKey({ key: publicJwk, format: "jwk" });
const keyObjectP1363 = crypto.sign("sha256", data, { key: privateKey, dsaEncoding: "ieee-p1363" });

const signer = crypto.createSign("sha256");
signer.update(data);
const streamingP1363 = signer.sign({ key: jwkKey, format: "jwk", dsaEncoding: "ieee-p1363" });

const verifier = crypto.createVerify("sha256");
verifier.update(data);

console.log("ec jwk p1363 len:", directP1363.length);
console.log("ec jwk der variable:", directDer.length > 0 && directDer.length <= 72 && directDer.length !== 64);
console.log("ec jwk p1363 verify direct:", crypto.verify("sha256", data, { key: publicJwk, format: "jwk", dsaEncoding: "ieee-p1363" }, directP1363));
console.log("ec jwk p1363 verify keyobject:", crypto.verify("sha256", data, { key: publicKey, dsaEncoding: "ieee-p1363" }, keyObjectP1363));
console.log("ec jwk p1363 streaming len:", streamingP1363.length);
console.log("ec jwk p1363 streaming verify:", verifier.verify({ key: publicJwk, format: "jwk", dsaEncoding: "ieee-p1363" }, streamingP1363));
