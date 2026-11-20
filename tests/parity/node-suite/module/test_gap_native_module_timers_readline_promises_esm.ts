// import of "timers/promises" and "readline/promises" (namespace, default, named).
import * as tp from "timers/promises";
import tpDefault from "node:timers/promises";
import { setTimeout as sleep, setInterval } from "timers/promises";
import * as rlp from "readline/promises";
import rlpDefault from "node:readline/promises";
import { createInterface } from "readline/promises";
import { promises as timersPromises } from "timers";
import readline from "readline";
import { PassThrough } from "stream";

const readlinePromises = readline.promises;

for (const n of ["setTimeout", "setImmediate", "setInterval"]) {
  console.log("timers/promises", n, typeof tp[n], typeof tpDefault[n], tp[n] === tpDefault[n]);
}
console.log("named", sleep === tp.setTimeout, setInterval === tp.setInterval);
console.log("timers.promises", typeof timersPromises, typeof timersPromises.setTimeout);
console.log("namespace", await tp.setTimeout(5, "ns"));
console.log("default", await tpDefault.setTimeout(5, "default"));
console.log("named call", await sleep(5, "named"));
console.log("comma", await (0, tp.setImmediate)("comma"));
console.log("scheduler", typeof tp.scheduler.wait, await tp.scheduler.wait(5));
let ticks = 0;
for await (const v of setInterval(5, "tick")) { ticks += 1; if (ticks === 2) break; }
console.log("interval", ticks);

for (const n of ["createInterface", "Interface", "Readline"]) {
  console.log("readline/promises", n, typeof rlp[n], typeof rlpDefault[n], rlp[n] === readlinePromises[n]);
}
console.log("named", createInterface === readlinePromises.createInterface);
const ask = async (make) => {
  const input = new PassThrough();
  const rl = make({ input, output: new PassThrough() });
  const answer = rl.question("q? ");
  input.write("esm\n");
  const value = await answer;
  rl.close();
  return value;
};
console.log("question ns", await ask((o) => rlp.createInterface(o)));
console.log("question default", await ask((o) => rlpDefault.createInterface(o)));
console.log("question named", await ask((o) => createInterface(o)));
process.exit(0);
