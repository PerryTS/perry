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
console.log(typeof server.context.require);
const contextModule = server.context.module;
console.log(typeof contextModule, contextModule?.id);
console.log(typeof server.context.Buffer, typeof server.context.process);
console.log(typeof server.context.fs, typeof server.context.path);
