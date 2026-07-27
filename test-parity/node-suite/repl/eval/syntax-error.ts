import { Recoverable, start } from "node:repl";

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
server.eval("const = 1\n", server.context, "fixture", (error: any) => {
  console.log(error instanceof SyntaxError, error instanceof Recoverable);
  console.log(error?.name);
});
