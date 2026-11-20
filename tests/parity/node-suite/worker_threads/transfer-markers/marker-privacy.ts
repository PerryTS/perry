import {
  isMarkedAsUntransferable,
  markAsUncloneable,
  markAsUntransferable,
} from "node:worker_threads";

function cloneOutcome(value: any): string {
  try {
    structuredClone(value);
    return "ok";
  } catch (error: any) {
    return `${error?.name}:${error?.code ?? ""}`;
  }
}

const marked: any = { value: 1 };
markAsUncloneable(marked);
console.log(
  "private clone mark:",
  Object.getOwnPropertySymbols(marked).length,
  Reflect.ownKeys(marked).join(","),
  cloneOutcome(marked),
);

delete marked[Symbol.for("nodejs.worker_threads.uncloneable")];
for (const symbol of Object.getOwnPropertySymbols(marked)) {
  delete marked[symbol];
}
console.log("permanent:", cloneOutcome(marked));

const forged: any = { value: 2 };
forged[Symbol.for("nodejs.worker_threads.uncloneable")] = true;
forged.isUncloneable = true;
console.log("forged clone:", cloneOutcome(forged));

const forgedBuffer: any = new ArrayBuffer(4);
forgedBuffer[Symbol.for("nodejs.worker_threads.untransferable")] = true;
console.log("forged transfer mark:", isMarkedAsUntransferable(forgedBuffer));

const realBuffer = new ArrayBuffer(4);
markAsUntransferable(realBuffer);
console.log(
  "private transfer mark:",
  Object.getOwnPropertySymbols(realBuffer).length,
  isMarkedAsUntransferable(realBuffer),
);
