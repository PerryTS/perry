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
server.context.value = 40;
server.eval(
  "value + 2\n",
  server.context,
  "fixture",
  (error: unknown, value: unknown) => {
    console.log(error === null, value);
  },
);
