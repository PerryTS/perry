import * as readline from "node:readline";

const prototype = (readline as any).Interface?.prototype;
if (prototype) {
  const descriptor = Object.getOwnPropertyDescriptor(prototype, "constructor");
  console.log(
    descriptor?.writable,
    descriptor?.enumerable,
    descriptor?.configurable,
  );
} else {
  console.log("missing");
}
