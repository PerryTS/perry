import { start } from "node:repl";

const input = {
  on() {},
  once() {},
  resume() {},
  pause() {},
  setEncoding() {},
  removeListener() {},
};
const output = {
  write() {
    return true;
  },
  on() {},
  once() {},
  removeListener() {},
  isTTY: false,
};
const server = start({ input, output, terminal: false, useGlobal: false });
console.log(server.context !== globalThis);
console.log(server.context.global === server.context);
console.log(server.context.Object !== Object);
console.log(server.context.console !== console);
