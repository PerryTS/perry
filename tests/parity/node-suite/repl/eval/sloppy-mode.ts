import { REPL_MODE_SLOPPY, start } from "node:repl";

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
const server = start({
  input,
  output,
  terminal: false,
  replMode: REPL_MODE_SLOPPY,
});
server.eval(
  "undeclaredSloppy = 7\n",
  server.context,
  "fixture",
  (error: unknown, value: unknown) => {
    console.log(error === null, value, server.context.undeclaredSloppy);
  },
);
