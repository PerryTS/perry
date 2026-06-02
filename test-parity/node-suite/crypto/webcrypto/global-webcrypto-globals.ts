const webcrypto = process.getBuiltinModule("node:crypto").webcrypto;
const globalCrypto = globalThis.crypto;
const subtle = globalCrypto.subtle;

const descriptorShape = (desc: any) => ({
  enumerable: desc?.enumerable,
  configurable: desc?.configurable,
  writable: "writable" in desc ? desc.writable : undefined,
  get: typeof desc?.get,
  set: typeof desc?.set,
  value: typeof desc?.value,
});

const throwsShape = (label: string, fn: () => void) => {
  try {
    fn();
    console.log(`${label}: no throw`);
  } catch (error: any) {
    console.log(`${label}:`, error.name, error.code ?? "");
  }
};

console.log("global crypto typeof:", typeof globalCrypto);
console.log("global crypto same as webcrypto:", globalCrypto === webcrypto);
console.log("bare crypto same as global:", crypto === globalCrypto);
console.log("Crypto typeof:", typeof Crypto);
console.log("CryptoKey typeof:", typeof CryptoKey);
console.log("SubtleCrypto typeof:", typeof SubtleCrypto);
console.log("crypto ctor identity:", globalCrypto.constructor === Crypto);
console.log("crypto proto ctor identity:", Object.getPrototypeOf(globalCrypto).constructor === Crypto);
console.log("crypto instanceof Crypto:", globalCrypto instanceof Crypto);
console.log("subtle typeof:", typeof subtle);
console.log("subtle same as webcrypto:", subtle === webcrypto.subtle);
console.log("subtle ctor identity:", subtle.constructor === SubtleCrypto);
console.log("subtle proto ctor identity:", Object.getPrototypeOf(subtle).constructor === SubtleCrypto);
console.log("subtle instanceof SubtleCrypto:", subtle instanceof SubtleCrypto);
console.log("crypto tag:", Object.prototype.toString.call(globalCrypto));
console.log("subtle tag:", Object.prototype.toString.call(subtle));
console.log("global crypto desc:", JSON.stringify(descriptorShape(Object.getOwnPropertyDescriptor(globalThis, "crypto"))));
console.log("global Crypto desc:", JSON.stringify(descriptorShape(Object.getOwnPropertyDescriptor(globalThis, "Crypto"))));
console.log("global CryptoKey desc:", JSON.stringify(descriptorShape(Object.getOwnPropertyDescriptor(globalThis, "CryptoKey"))));
console.log("global SubtleCrypto desc:", JSON.stringify(descriptorShape(Object.getOwnPropertyDescriptor(globalThis, "SubtleCrypto"))));
console.log("Crypto.prototype.subtle desc:", JSON.stringify(descriptorShape(Object.getOwnPropertyDescriptor(Crypto.prototype, "subtle"))));
console.log("Crypto.prototype.getRandomValues desc:", JSON.stringify(descriptorShape(Object.getOwnPropertyDescriptor(Crypto.prototype, "getRandomValues"))));
console.log("Crypto.prototype.randomUUID desc:", JSON.stringify(descriptorShape(Object.getOwnPropertyDescriptor(Crypto.prototype, "randomUUID"))));
console.log("CryptoKey.prototype.algorithm desc:", JSON.stringify(descriptorShape(Object.getOwnPropertyDescriptor(CryptoKey.prototype, "algorithm"))));

throwsShape("new Crypto", () => new (Crypto as any)());
throwsShape("new CryptoKey", () => new (CryptoKey as any)());
throwsShape("new SubtleCrypto", () => new (SubtleCrypto as any)());

const bytes = new Uint8Array(8);
const filled = globalCrypto.getRandomValues(bytes);
console.log("getRandomValues same object:", filled === bytes);
console.log("getRandomValues length:", filled.length);
const reboundGetRandomValues = (globalCrypto as any)["getRandomValues"];
throwsShape("rebound getRandomValues", () =>
  Reflect.apply(reboundGetRandomValues, undefined, [new Uint8Array(1)]),
);

const uuid = globalCrypto.randomUUID();
console.log(
  "randomUUID shape:",
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(uuid),
);
const reboundRandomUUID = (globalCrypto as any)["randomUUID"];
throwsShape("rebound randomUUID", () => Reflect.apply(reboundRandomUUID, undefined, []));

const key = await subtle.generateKey({ name: "AES-GCM", length: 128 }, true, ["encrypt", "decrypt"]);
console.log("key instanceof CryptoKey:", key instanceof CryptoKey);
console.log("key ctor identity:", key.constructor === CryptoKey);
console.log("key tag:", Object.prototype.toString.call(key));
