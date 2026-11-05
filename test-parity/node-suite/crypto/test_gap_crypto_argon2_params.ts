import { argon2, argon2Sync } from "node:crypto";

const base = {
  message: Buffer.from("password"),
  nonce: Buffer.from("0123456789abcdef"),
  parallelism: 1,
  tagLength: 16,
  memory: 8,
  passes: 1,
};

const cases = [
  { label: "plain", options: {}, expected: "83947cb26635e8b2500b348925b220dc" },
  { label: "secret", options: { secret: Buffer.from("pepper") }, expected: "3e72ec885ed9f4637910e1e9aaee403b" },
  { label: "ad", options: { associatedData: Buffer.from("ad") }, expected: "3741472183da524860bdccc2d27028e1" },
  { label: "both", options: { secret: Buffer.from("pepper"), associatedData: Buffer.from("ad") }, expected: "d7aab58ed8561dda350519e5d7d7f2a5" },
  { label: "empty-secret", options: { secret: Buffer.alloc(0) }, expected: "83947cb26635e8b2500b348925b220dc" },
  { label: "empty-ad", options: { associatedData: Buffer.alloc(0) }, expected: "83947cb26635e8b2500b348925b220dc" },
  { label: "ad32", options: { associatedData: Buffer.alloc(32, 97) }, expected: "91650bb6d6854c1602cd531d13b31c9c" },
  { label: "empty", options: { secret: Buffer.alloc(0), associatedData: Buffer.alloc(0) }, expected: "83947cb26635e8b2500b348925b220dc" },
  { label: "undefined", options: { secret: undefined, associatedData: undefined }, expected: "83947cb26635e8b2500b348925b220dc" },
];

for (const item of cases) {
  const key = argon2Sync("argon2id", { ...base, ...item.options });
  console.log(item.label, "sync", Buffer.isBuffer(key), key.length, key.toString("hex") === item.expected);
  await new Promise<void>((resolve) => {
    argon2("argon2id", { ...base, ...item.options }, (err, result) => {
      console.log(item.label, "async", err === null, Buffer.isBuffer(result), result.length, result.toString("hex") === item.expected);
      resolve();
    });
  });
}

// Every documented byte-input shape must read exactly the view's slice.
const secretBytes = new Uint8Array([0, 112, 101, 112, 112, 101, 114, 0]);
const adBytes = new Uint8Array([0, 97, 100, 0]);
const shapes = [
  { label: "string", secret: "pepper", associatedData: "ad" },
  { label: "typed", secret: secretBytes.subarray(1, 7), associatedData: adBytes.subarray(1, 3) },
  { label: "arraybuffer", secret: secretBytes.slice(1, 7).buffer, associatedData: adBytes.slice(1, 3).buffer },
  { label: "dataview", secret: new DataView(secretBytes.buffer, 1, 6), associatedData: new DataView(adBytes.buffer, 1, 2) },
];
for (const shape of shapes) {
  const result = argon2Sync("argon2id", { ...base, secret: shape.secret, associatedData: shape.associatedData });
  console.log(shape.label, result.toString("hex") === "d7aab58ed8561dda350519e5d7d7f2a5");
  await new Promise<void>((resolve) => {
    argon2("argon2id", { ...base, secret: shape.secret, associatedData: shape.associatedData }, (err, key) => {
      console.log(shape.label, "async", err === null, key.toString("hex") === "d7aab58ed8561dda350519e5d7d7f2a5");
      resolve();
    });
  });
}

for (const field of ["secret", "associatedData"]) {
  for (const invalid of [null, 42, true, {}]) {
    const params = { ...base, [field]: invalid };
    try {
      argon2Sync("argon2id", params);
      console.log(field, "sync invalid accepted");
    } catch (err) {
      console.log(field, "sync", err.name, err.code);
    }
    try {
      argon2("argon2id", params, () => console.log("invalid callback ran"));
      console.log(field, "async invalid accepted");
    } catch (err) {
      console.log(field, "async", err.name, err.code);
    }
  }
}

// The worker must use a submission-time copy, not borrowed JS heap bytes.
const secret = Buffer.from("pepper");
const associatedData = Buffer.from("ad");
await new Promise<void>((resolve) => {
  argon2("argon2id", { ...base, secret, associatedData }, (err, key) => {
    console.log("owned async", err === null, key.toString("hex") === "d7aab58ed8561dda350519e5d7d7f2a5");
    resolve();
  });
  secret.fill(0);
  associatedData.fill(0);
});
