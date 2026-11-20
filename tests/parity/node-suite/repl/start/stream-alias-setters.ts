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
const server = start({ input, output, terminal: false });
const nextInput = { marker: "input", pause() {} };
const nextOutput = { marker: "output" };
server.inputStream = nextInput as any;
server.outputStream = nextOutput as any;
console.log(server.input === nextInput, server.inputStream === nextInput);
console.log(server.output === nextOutput, server.outputStream === nextOutput);
