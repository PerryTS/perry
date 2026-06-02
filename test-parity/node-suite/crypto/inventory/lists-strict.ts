import * as crypto from "node:crypto";

function stringsOnly(list: string[]) {
  return list.every((value) => typeof value === "string");
}

const hashes = crypto.getHashes();
console.log("hashes strings:", stringsOnly(hashes));
console.log("hashes has lowercase sha1:", hashes.includes("sha1"));
console.log("hashes no uppercase sha1:", hashes.includes("SHA1"));
console.log("hashes has RSA-SHA1:", hashes.includes("RSA-SHA1"));
console.log("hashes no rsa-sha1:", hashes.includes("rsa-sha1"));
const hashesBefore = crypto.getHashes().join("|");
hashes.push("some-arbitrary-value");
console.log("hashes immutable:", crypto.getHashes().join("|") === hashesBefore);

const ciphers = crypto.getCiphers();
console.log("ciphers strings:", stringsOnly(ciphers));
console.log("ciphers has aes-128-cbc:", ciphers.includes("aes-128-cbc"));
const ciphersBefore = crypto.getCiphers().join("|");
ciphers.push("some-arbitrary-value");
console.log("ciphers immutable:", crypto.getCiphers().join("|") === ciphersBefore);

const curves = crypto.getCurves();
console.log("curves strings:", stringsOnly(curves));
console.log("curves has secp384r1:", curves.includes("secp384r1"));
console.log("curves no uppercase secp384r1:", curves.includes("SECP384R1"));
const curvesBefore = crypto.getCurves().join("|");
curves.push("some-arbitrary-value");
console.log("curves immutable:", crypto.getCurves().join("|") === curvesBefore);
