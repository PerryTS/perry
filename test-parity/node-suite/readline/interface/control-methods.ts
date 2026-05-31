import * as readline from "node:readline";
import { PassThrough, Writable } from "node:stream";

const input = new PassThrough();
const writes: string[] = [];
const output = new Writable({
  write(chunk, _encoding, callback) {
    writes.push(String(chunk));
    callback();
  },
});

const rl = readline.createInterface({
  input,
  output,
  terminal: false,
  prompt: "p> ",
});

console.log("initial prompt:", JSON.stringify(rl.getPrompt()));
console.log("initial terminal:", rl.terminal);
console.log("initial line:", JSON.stringify(rl.line));
rl.setPrompt("q> ");
console.log("updated prompt:", JSON.stringify(rl.getPrompt()));
console.log("pause ret same:", rl.pause() === rl);
console.log("resume ret same:", rl.resume() === rl);
console.log("prompt ret:", rl.prompt() === undefined);
console.log("write ret:", rl.write("xy") === undefined);
console.log("cursor pos keys:", Object.keys(rl.getCursorPos()).join(","));
console.log("cursor pos:", JSON.stringify(rl.getCursorPos()));
rl.close();
console.log("writes:", JSON.stringify(writes.join("")));
