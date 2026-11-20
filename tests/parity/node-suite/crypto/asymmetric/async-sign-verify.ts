import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

function signAsync(algorithm: string | undefined, data: Buffer, key: any): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    const ret = crypto.sign(algorithm as any, data, key, (err: Error | null, signature: Buffer) => {
      if (err) reject(err);
      else resolve(signature);
    });
    console.log("sign ret undefined:", ret === undefined);
  });
}

function verifyAsync(algorithm: string | undefined, data: Buffer, key: any, signature: Buffer): Promise<boolean> {
  return new Promise((resolve, reject) => {
    const ret = crypto.verify(algorithm as any, data, key, signature, (err: Error | null, ok: boolean) => {
      if (err) reject(err);
      else resolve(ok);
    });
    console.log("verify ret undefined:", ret === undefined);
  });
}

async function main() {
  const data = Buffer.from("async sign verify data");

  const rsa = crypto.generateKeyPairSync("rsa", { modulusLength: 2048 });
  const rsaSig = await signAsync("sha256", data, rsa.privateKey);
  console.log("rsa async sig len:", rsaSig.length);
  console.log("rsa async verify ok:", await verifyAsync("sha256", data, rsa.publicKey, rsaSig));
  console.log("rsa async verify bad:", await verifyAsync("sha256", Buffer.from("bad"), rsa.publicKey, rsaSig));

  const ec = crypto.generateKeyPairSync("ec", { namedCurve: "P-256" });
  const ecSig = await signAsync("sha256", data, { key: ec.privateKey, dsaEncoding: "ieee-p1363" });
  console.log("ec async sig len:", ecSig.length);
  console.log("ec async verify ok:", await verifyAsync("sha256", data, { key: ec.publicKey, dsaEncoding: "ieee-p1363" }, ecSig));

  const ed = crypto.generateKeyPairSync("ed25519");
  const edSig = await signAsync(undefined, data, ed.privateKey);
  console.log("ed async sig len:", edSig.length);
  console.log("ed async verify ok:", await verifyAsync(undefined, data, ed.publicKey, edSig));
}

await main();
