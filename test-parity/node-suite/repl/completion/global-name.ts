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
server.completer("conso", (error: unknown, result: [string[], string]) => {
  console.log(error === null);
  console.log(result[0].includes("console"), result[1]);
});
