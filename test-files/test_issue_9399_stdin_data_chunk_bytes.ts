// #9399 / #9400: `process.stdin` delivers its bytes.
//
// The parity runner gives a fixture no stdin, so this test re-spawns itself
// with a pipe on the child's stdin and drives each shape in a child role.
//
// What is under test: `stdin_chunk_jsvalue` built the `'data'` chunk with
// `buffer_alloc(len)`, which reserves CAPACITY but leaves `length` at 0, and
// never set it. Every chunk delivered as a Buffer — Node's default, i.e.
// whenever `setEncoding` has NOT been called — therefore arrived EMPTY:
// `chunk.length === 0`, `toString()` was `""`, `Buffer.concat` appended
// nothing, `JSON.stringify(chunk)` reported `{"type":"Buffer","data":[]}`.
// The bytes were copied into the payload; nothing could see them.
//
// claude-code's MCP stdio transport does `readBuffer.append(chunk)` on raw
// (un-encoded) chunks, so its newline-delimited JSON-RPC framer never saw a
// byte and `readMessage()` returned null forever — `claude mcp serve` answered
// nothing. The `setEncoding("utf8")` role below is the control: that path built
// a string instead of a Buffer and was never affected.
//
// Each role holds a short interval while it reads. That is deliberate and NOT
// part of what is being tested: a stdin listener registered on the stdin OBJECT
// (an alias / parameter / field, rather than the literal `process.stdin.on(...)`
// spelling codegen lowers to a readline extern) does not reliably hold perry's
// event loop open, so without a timer the process sometimes exits before the
// pump delivers. That race is tracked separately; pinning the loop here keeps
// this fixture measuring the chunk contents, deterministically.
import { spawn } from "node:child_process";

const ROLE_ENV = "PERRY_9399_STDIN_ROLE";
const PAYLOAD = "alpha\nbeta\n";
const role = process.env[ROLE_ENV] ?? "";

// Keep the loop alive for the duration of the read, then release it.
function holdLoop(): void {
  const ticker = setInterval(() => {}, 20);
  setTimeout(() => clearInterval(ticker), 500);
}

if (role === "chunk") {
  const stream = process.stdin;
  const chunks: Buffer[] = [];
  let total = 0;
  let sawNonBuffer = false;
  const onData = (chunk: Buffer) => {
    if (!Buffer.isBuffer(chunk)) sawNonBuffer = true;
    total += chunk.length;
    chunks.push(chunk);
  };
  stream.on("data", onData);
  stream.on("end", () => {
    console.log("chunk bytes:", total);
    console.log("chunk nonBuffer:", sawNonBuffer);
    console.log("chunk text:", JSON.stringify(Buffer.concat(chunks).toString("utf8")));
  });
  holdLoop();
} else if (role === "encoded") {
  const stream = process.stdin;
  stream.setEncoding("utf8");
  let acc = "";
  stream.on("data", (chunk: string) => {
    acc += chunk;
  });
  stream.on("end", () => {
    console.log("encoded text:", JSON.stringify(acc));
  });
  holdLoop();
} else if (role === "await") {
  holdLoop();
  (async () => {
    let acc = "";
    let count = 0;
    for await (const chunk of process.stdin) {
      acc += String(chunk);
      count += 1;
    }
    console.log("await nonEmpty:", count > 0);
    console.log("await text:", JSON.stringify(acc));
  })();
} else {
  const childArgs = [...process.execArgv, ...process.argv.slice(1)];
  const runRole = (name: string) =>
    new Promise<void>((resolve) => {
      const child = spawn(process.execPath, childArgs, {
        env: { ...process.env, [ROLE_ENV]: name },
        stdio: ["pipe", "inherit", "inherit"],
      });
      child.on("exit", (code) => {
        console.log(name + " exit:", code);
        resolve();
      });
      child.stdin!.write(PAYLOAD);
      child.stdin!.end();
    });

  (async () => {
    await runRole("chunk");
    await runRole("encoded");
    await runRole("await");
    console.log("done");
  })();
}
