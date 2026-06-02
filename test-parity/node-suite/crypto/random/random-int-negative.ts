import { randomInt } from "node:crypto";

const neg = randomInt(-10, -2);
console.log("negative range ok:", neg >= -10 && neg < -2);
const mixed = randomInt(-2, 3);
console.log("mixed range ok:", mixed >= -2 && mixed < 3);

const asyncNeg = await new Promise<number>((resolve, reject) => {
  const ret = randomInt(-5, -1, (err, n) => {
    if (err) reject(err);
    else resolve(n);
  });
  console.log("negative async ret undefined:", ret === undefined);
});
console.log("negative async range ok:", asyncNeg >= -5 && asyncNeg < -1);
