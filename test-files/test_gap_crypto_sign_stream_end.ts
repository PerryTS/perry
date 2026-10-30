// Sign and Verify are Writable streams: `write` and `end` feed the signed data.
import { createSign, createVerify, generateKeyPairSync } from "node:crypto";

const { privateKey: key, publicKey } = generateKeyPairSync("ec", { namedCurve: "P-256" });
const data = "header.payload";

const signer = createSign("sha256");
console.log("typeof write", typeof signer.write, "typeof end", typeof signer.end);
console.log("end returns signer", signer.update(data).end() === signer);
const sig = signer.sign({ key, dsaEncoding: "ieee-p1363" });
console.log("ieee-p1363 length", sig.length);

const verifier = createVerify("sha256");
console.log("end returns verifier", verifier.update(data).end() === verifier);
console.log("verify after end", verifier.verify({ key: publicKey, dsaEncoding: "ieee-p1363" }, sig));

const written = createSign("sha256");
console.log("write returns", written.write("header."));
written.end("payload");
const writtenSig = written.sign(key);
const writtenVerifier = createVerify("sha256");
writtenVerifier.write("header.");
writtenVerifier.end(Buffer.from("payload"));
console.log("write/end chunks verify", writtenVerifier.verify(publicKey, writtenSig));

const rsa = generateKeyPairSync("rsa", { modulusLength: 2048 });
const rsaSig = createSign("RSA-SHA256").update(data).end().sign(rsa.privateKey);
console.log("rsa verify", createVerify("RSA-SHA256").update(data).verify(rsa.publicKey, rsaSig));

