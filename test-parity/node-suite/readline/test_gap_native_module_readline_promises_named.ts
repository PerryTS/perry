// `import { promises } from "readline"` binds the readline/promises namespace, the
// same object `readline.promises` reads, with every export the real implementation.
import { promises } from "readline";
import { promises as nodePrefixed } from "node:readline";
import * as rlp from "readline/promises";
import { PassThrough } from "stream";

console.log("promises", typeof promises, promises === nodePrefixed, promises === rlp.default || promises === rlp);
for (const n of ["createInterface", "Interface", "Readline"]) {
  console.log("export", n, typeof promises[n], promises[n] === rlp[n]);
}
const input = new PassThrough();
const rl = promises.createInterface({ input, output: new PassThrough() });
const answer = rl.question("q? ");
input.write("named\n");
console.log("question", await answer);
rl.close();
process.exit(0);
