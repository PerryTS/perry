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
const server = start({ input, output, terminal: false });
console.log(
  server.terminal,
  server.useColors,
  server.useGlobal,
  server.ignoreUndefined,
);
console.log(
  server.editorMode,
  server.breakEvalOnSigint,
  server.allowBlockingCompletions,
);
console.log(server.replMode === REPL_MODE_SLOPPY, server.historySize);
