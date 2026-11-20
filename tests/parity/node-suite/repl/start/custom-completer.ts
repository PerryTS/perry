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
const completer = (_line: string, callback: Function) =>
  callback(null, [[], ""]);
const server = start({ input, output, terminal: false, completer });
console.log(server.completer === completer);
