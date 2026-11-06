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
try {
  server.defineCommand("bad", { action: 1 } as any);
} catch (error: any) {
  console.log(error.name, error.code);
  console.log(error.message);
}
