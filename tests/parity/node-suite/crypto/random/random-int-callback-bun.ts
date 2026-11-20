import { randomInt } from "node:crypto";

const result = await new Promise<string>((resolve) => {
  const ret = randomInt(1, 2, (err, num) => {
    resolve([
      "randomInt callback return undefined: " + (ret === undefined),
      "randomInt callback err nullish: " + (err == null),
      "randomInt callback num: " + num,
    ].join("\n"));
  });
});
console.log(result);
