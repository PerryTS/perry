import { randomInt } from "node:crypto";

function randomIntAsync(min: number, max: number): Promise<number> {
  return new Promise((resolve, reject) => {
    const ret = randomInt(min, max, (err: Error | null, n: number) => {
      if (err) reject(err);
      else resolve(n);
    });
    console.log("randomInt ret undefined:", ret === undefined);
  });
}

const n = await randomIntAsync(5, 10);
console.log("randomInt async in range:", n >= 5 && n < 10);
