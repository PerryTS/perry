const webcrypto = process.getBuiltinModule("node:crypto").webcrypto;
const globalCrypto = globalThis.crypto;
const subtle = globalCrypto.subtle;

console.log("global crypto typeof:", typeof globalCrypto);
console.log("global crypto same as webcrypto:", globalCrypto === webcrypto);
console.log("bare crypto same as global:", crypto === globalCrypto);
console.log("Crypto typeof:", typeof Crypto);
console.log("CryptoKey typeof:", typeof CryptoKey);
console.log("SubtleCrypto typeof:", typeof SubtleCrypto);
console.log("crypto ctor identity:", globalCrypto.constructor === Crypto);
console.log("crypto proto ctor identity:", Object.getPrototypeOf(globalCrypto).constructor === Crypto);
console.log("subtle typeof:", typeof subtle);
console.log("subtle same as webcrypto:", subtle === webcrypto.subtle);
console.log("subtle ctor identity:", subtle.constructor === SubtleCrypto);
console.log("subtle proto ctor identity:", Object.getPrototypeOf(subtle).constructor === SubtleCrypto);
console.log("crypto tag:", Object.prototype.toString.call(globalCrypto));
console.log("subtle tag:", Object.prototype.toString.call(subtle));

const bytes = new Uint8Array(8);
const filled = globalCrypto.getRandomValues(bytes);
console.log("getRandomValues same object:", filled === bytes);
console.log("getRandomValues length:", filled.length);

const uuid = globalCrypto.randomUUID();
console.log(
  "randomUUID shape:",
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(uuid),
);
