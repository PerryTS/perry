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
const previous = server.context;
previous.marker = 42;
server.resetContext();
console.log(server.context !== previous);
console.log(server.context.marker);
console.log(server.context.global === server.context);
