import { start, REPL_MODE_STRICT } from "node:repl";

const input = {
  on() {},
  once() {},
  resume() {},
  pause() {},
  setEncoding() {},
  removeListener() {},
};
let output = "";
const out = {
  write(chunk: unknown) {
    output += String(chunk);
    return true;
  },
  on() {},
  once() {},
  removeListener() {},
  columns: 80,
  isTTY: false,
};

const server = start({
  input,
  output: out,
  terminal: false,
  prompt: "p> ",
  useColors: false,
  ignoreUndefined: true,
  replMode: REPL_MODE_STRICT,
});

server.context.value = 40;
console.log("server ctor:", server.constructor && server.constructor.name);
console.log(
  "server methods:",
  typeof server.write,
  typeof server.defineCommand,
  typeof server.displayPrompt,
  typeof server.clearBufferedCommand,
  typeof server.setupHistory,
);
console.log(
  "server flags:",
  server.editorMode,
  server.useColors,
  server.useGlobal,
  server.ignoreUndefined,
  typeof server.replMode,
  server.replMode === REPL_MODE_STRICT,
);
console.log("server context value:", server.context.value);

server.on("exit", () => {
  console.log("exit fired");
  console.log("captured:", JSON.stringify(output));
});

server.write("value + 2\n");
server.write("undefined\n");
server.write(".exit\n");
