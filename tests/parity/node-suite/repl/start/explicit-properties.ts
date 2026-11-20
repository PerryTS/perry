import { REPL_MODE_STRICT, start } from "node:repl";

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
  useColors: true,
  useGlobal: true,
  ignoreUndefined: true,
  breakEvalOnSigint: true,
  allowBlockingCompletions: true,
  replMode: REPL_MODE_STRICT,
  historySize: 47,
});
console.log(
  server.terminal,
  server.useColors,
  server.useGlobal,
  server.ignoreUndefined,
);
console.log(server.breakEvalOnSigint, server.allowBlockingCompletions);
console.log(server.replMode === REPL_MODE_STRICT, server.historySize);
