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
server.eval(
  "20 + 22\n",
  server.context,
  "fixture",
  (error: unknown, value: unknown) => {
    console.log(error === null, value);
  },
);
