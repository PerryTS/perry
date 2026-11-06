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
try {
  start({ input, output, terminal: false, breakEvalOnSigint: true, eval() {} });
} catch (error: any) {
  console.log(error.name, error.code);
  console.log(error.message);
}
