import { AsyncLocalStorage } from "node:async_hooks";
import { checkPrime, generatePrime } from "node:crypto";

const storage = new AsyncLocalStorage<string>();
const checked = await storage.run(
  "generate-prime",
  () =>
    new Promise<boolean>((resolve, reject) => {
      const generated = generatePrime(32, (generateError, prime) => {
        console.log("generatePrime store:", storage.getStore());
        if (generateError) return reject(generateError);
        storage.enterWith("check-prime");
        const checked = checkPrime(prime, (checkError, value) => {
          console.log("checkPrime store:", storage.getStore());
          if (checkError) return reject(checkError);
          resolve(value);
        });
        console.log("checkPrime return undefined:", checked === undefined);
      });
      console.log("generatePrime return undefined:", generated === undefined);
    }),
);
console.log("prime checked:", checked);
console.log("prime outside:", String(storage.getStore()));
