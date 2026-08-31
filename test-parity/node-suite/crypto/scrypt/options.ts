import * as crypto from "node:crypto";
import { promisify } from "node:util";

const password = "correct horse battery staple";
const salt = Buffer.from("0123456789abcdef0123456789abcdef", "hex");
const maxmem = 512 * 1024 * 1024;
const vectors: Array<[number, string]> = [
  [1 << 12, "05f47c22e65fc21d3e11a92222323c577271be35cea9b34b4a6970e2fa3ebf48"],
  [1 << 13, "a2d6acfcc30e33aa067080845b9f4790427da3d8f992e651765c095abb7cd276"],
  [1 << 14, "33e39503baad99447708713ceec3f39bb876329254d4f5cd93da92a65d983f01"],
  [1 << 15, "cb18e56f62485654eba440e7cc2fcaff9f92102d944dab86b13642272dfeb1f4"],
  [1 << 16, "9d12077809f9271ef0dd063bff62817d49d53c7f1acb8e42c67516c5b0287cf1"],
  [1 << 17, "e5581239883361913bc8cd281ef2a9d7e5bf2171f4b6c8eb0292e99077483208"],
];

function equal(actual: string, expected: string, label: string): void {
  if (actual !== expected) {
    throw new Error(label + ": expected " + expected + ", got " + actual);
  }
}

function requireScryptRangeError(error: any, fragment: string): void {
  if (error?.name !== "RangeError") {
    throw new Error("expected RangeError, got " + String(error));
  }
  if (error?.code !== "ERR_CRYPTO_INVALID_SCRYPT_PARAMS") {
    throw new Error("unexpected error code: " + String(error?.code));
  }
  if (!String(error).includes(fragment)) {
    throw new Error("unexpected error message: " + String(error));
  }
}

for (const [N, expected] of vectors) {
  equal(
    crypto.scryptSync(password, salt, 32, { N, r: 8, p: 1, maxmem }).toString("hex"),
    expected,
    "scryptSync N=" + N,
  );
}
equal(
  crypto.scryptSync(password, salt, 32, { N: 1 << 14, r: 4, p: 1, maxmem }).toString("hex"),
  "cd4b3b2db0af02f65f71c999901e07abba66fd088d65268928a7e971a50651f0",
  "scryptSync r=4",
);
equal(
  crypto.scryptSync(password, salt, 32, { N: 1 << 14, r: 8, p: 2, maxmem }).toString("hex"),
  "77636ab3d38dd67285438f5694c64bfa9fe81144034a49e7599511136bcfb071",
  "scryptSync p=2",
);
equal(
  crypto.scryptSync(password, salt, 64, { N: 1 << 12, r: 8, p: 1, maxmem }).toString("hex"),
  "05f47c22e65fc21d3e11a92222323c577271be35cea9b34b4a6970e2fa3ebf48" +
    "1399c1db6db41b7c49fcc677fbe03319ec8e9c37a73734dd9eec71bd1d9ed212",
  "scryptSync keylen=64",
);
equal(
  crypto.scryptSync(password, salt, 32, {
    cost: 1 << 12,
    blockSize: 8,
    parallelization: 1,
    maxmem,
  }).toString("hex"),
  vectors[0][1],
  "scryptSync aliases",
);
console.log("scryptSync parameter vectors: ok");

const scrypt = promisify(crypto.scrypt);
for (const [N, expected] of vectors) {
  const result: any = await scrypt(password, salt, 32, { N, r: 8, p: 1, maxmem });
  equal(result.toString("hex"), expected, "scrypt async N=" + N);
}
console.log("scrypt async parameter vectors: ok");

await new Promise<void>((resolve, reject) => {
  crypto.scrypt(password, salt, 32, { N: 1 << 14, r: 4, p: 1, maxmem }, (error, key) => {
    if (error) {
      reject(error);
      return;
    }
    try {
      equal(
        key.toString("hex"),
        "cd4b3b2db0af02f65f71c999901e07abba66fd088d65268928a7e971a50651f0",
        "scrypt callback r=4",
      );
      resolve();
    } catch (caught) {
      reject(caught);
    }
  });
});
console.log("scrypt direct callback options: ok");

try {
  crypto.scryptSync(password, salt, 32, { N: 3, r: 8, p: 1, maxmem });
  throw new Error("invalid N did not throw");
} catch (error: any) {
  requireScryptRangeError(error, "Invalid scrypt params");
}

try {
  await scrypt(password, salt, 32, { N: 1 << 17, r: 8, p: 1, maxmem: 1024 });
  throw new Error("insufficient maxmem did not throw");
} catch (error: any) {
  requireScryptRangeError(error, "memory limit exceeded");
}
console.log("scrypt invalid parameter errors: ok");
