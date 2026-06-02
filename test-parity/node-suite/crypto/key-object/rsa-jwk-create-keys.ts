import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const pair = await crypto.subtle.generateKey(
    { name: "RSASSA-PKCS1-v1_5", modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" },
    true,
    ["sign", "verify"],
  );

  const publicJwk = await crypto.subtle.exportKey("jwk", pair.publicKey) as JsonWebKey;
  const privateJwk = await crypto.subtle.exportKey("jwk", pair.privateKey) as JsonWebKey;

  const privateKey = crypto.createPrivateKey({ key: privateJwk, format: "jwk" });
  const publicKey = crypto.createPublicKey({ key: publicJwk, format: "jwk" });
  const derivedPublicKey = crypto.createPublicKey({ key: privateJwk, format: "jwk" });

  const data = Buffer.from("rsa jwk create keys parity");
  const signature = crypto.sign("RSA-SHA256", data, privateKey);
  const encrypted = crypto.publicEncrypt(publicKey, data);
  const decrypted = crypto.privateDecrypt(privateKey, encrypted);
  const directSig = crypto.sign("RSA-SHA256", data, { key: privateJwk, format: "jwk" });
  const directEncrypted = crypto.publicEncrypt({ key: publicJwk, format: "jwk" }, data);
  const directEncryptedWithPrivate = crypto.publicEncrypt({ key: privateJwk, format: "jwk" }, data);

  console.log("rsa jwk public kty:", publicJwk.kty);
  console.log("rsa jwk private has crt:", !!privateJwk.p, !!privateJwk.q);
  console.log("rsa jwk sig len:", signature.length);
  console.log("rsa jwk verify public:", crypto.verify("RSA-SHA256", data, publicKey, signature));
  console.log("rsa jwk verify derived:", crypto.verify("RSA-SHA256", data, derivedPublicKey, signature));
  console.log("rsa jwk verify direct:", crypto.verify("RSA-SHA256", data, { key: publicJwk, format: "jwk" }, directSig));
  console.log("rsa jwk decrypt:", decrypted.toString());
  console.log("rsa jwk direct decrypt:", crypto.privateDecrypt({ key: privateJwk, format: "jwk" }, directEncrypted).toString());
  console.log("rsa jwk direct private decrypt:", crypto.privateDecrypt({ key: privateJwk, format: "jwk" }, directEncryptedWithPrivate).toString());
}

await main();
