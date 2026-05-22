import { generatePrimeSync, checkPrimeSync, generatePrime, checkPrime } from "node:crypto";
import { Buffer } from "node:buffer";

function generatePrimeAsync(size: number, options: any = {}): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    const ret = generatePrime(size, options, (err: Error | null, prime: Buffer) => {
      if (err) reject(err);
      else resolve(prime);
    });
    console.log("generatePrime ret undefined:", ret === undefined);
  });
}

function checkPrimeAsync(candidate: Buffer, options: any = {}): Promise<boolean> {
  return new Promise((resolve, reject) => {
    const ret = checkPrime(candidate, options, (err: Error | null, ok: boolean) => {
      if (err) reject(err);
      else resolve(ok);
    });
    console.log("checkPrime ret undefined:", ret === undefined);
  });
}

const prime = generatePrimeSync(32);
console.log("prime len:", Buffer.from(prime).length);
console.log("prime check sync:", checkPrimeSync(prime));

const safePrime = generatePrimeSync(32, { safe: true });
console.log("safe prime check sync:", checkPrimeSync(safePrime));

const add = Buffer.from([12]);
const rem = Buffer.from([11]);
const constrained = generatePrimeSync(32, { add, rem });
console.log("constrained check sync:", checkPrimeSync(constrained));
console.log("constrained remainder:", Buffer.from(constrained).readUInt32BE() % 12);

const asyncPrime = await generatePrimeAsync(32, {});
console.log("async prime len:", Buffer.from(asyncPrime).length);
console.log("async check sync:", checkPrimeSync(asyncPrime));
console.log("async check callback:", await checkPrimeAsync(asyncPrime, {}));
console.log("known composite false:", checkPrimeSync(Buffer.from([0x00, 0x00, 0x00, 0x15])));
