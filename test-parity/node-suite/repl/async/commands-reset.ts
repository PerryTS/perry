import { start } from "node:repl";

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

const server = start({ input, output: out, terminal: false, prompt: "p> ", useColors: false });

server.defineCommand("hello", {
  help: "hello help",
  action(name: string) {
    console.log("action this server:", this === server);
    this.outputStream.write("hello " + name.trim() + "\n");
    this.displayPrompt();
  },
});

console.log("displayPrompt return:", server.displayPrompt(true) === undefined);
console.log("clearBuffered return:", server.clearBufferedCommand() === undefined);

server.on("reset", (ctx: any) => {
  console.log("reset event:", ctx === server.context, typeof ctx);
  ctx.afterReset = 7;
});
server.on("exit", () => {
  console.log("exit fired");
  console.log("captured:", JSON.stringify(output));
});

server.write(".hello world\n");
server.write(".clear\n");
server.write("afterReset + 1\n");
server.write(".exit\n");
