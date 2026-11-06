import { start } from "node:repl";

const stream = {
  write() {
    return true;
  },
  on() {},
  once() {},
  resume() {},
  pause() {},
  setEncoding() {},
  removeListener() {},
  isTTY: false,
};
const server = start({ stream, terminal: false } as any);
console.log(server.input === stream, server.output === stream);
console.log(server.inputStream === stream, server.outputStream === stream);
