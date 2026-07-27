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
const evaluate = (
  _code: string,
  _context: unknown,
  _filename: string,
  callback: Function,
) => callback(null, 1);
const server = start({ input, output, terminal: false, eval: evaluate });
console.log(typeof server.eval, server.eval === evaluate);
