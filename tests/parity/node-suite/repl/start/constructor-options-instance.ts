import { REPLServer } from "node:repl";

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
const server = new REPLServer({ input, output, terminal: false });
console.log(server instanceof REPLServer);
console.log(server.constructor === REPLServer);
