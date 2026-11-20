import { Console } from "node:console";
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
const server = start({ input, output, terminal: false, useGlobal: false });
const context = server.createContext();
console.log(context !== server.context);
console.log(context.global === context);
console.log(context.console instanceof Console);
console.log(typeof context.require, typeof context.module);
