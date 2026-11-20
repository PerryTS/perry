import { start } from "node:repl";

function run(ignoreUndefined: boolean) {
  const input = {
    on() {},
    once() {},
    resume() {},
    pause() {},
    setEncoding() {},
    removeListener() {},
  };
  let captured = "";
  const output = {
    write(chunk: unknown) {
      captured += String(chunk);
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
    prompt: "",
    useColors: false,
    ignoreUndefined,
  });
  server.write("undefined\n");
  return captured;
}

console.log(JSON.stringify(run(false)));
console.log(JSON.stringify(run(true)));
