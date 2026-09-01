// #9399 / #9400: `process.stdin` as a real Readable.
//
// The parity runner gives a fixture no stdin, so this test re-spawns itself
// with a pipe on the child's stdin and drives each shape in a child role.
//
// Roles:
//   chunk — an ALIASED `data` listener (`const s = process.stdin; s.on(...)`,
//           the spelling claude-code's MCP stdio transport uses via
//           `this._stdin.on("data", this._ondata)`). Two defects met here:
//             * `stdin_chunk_jsvalue` allocated the chunk Buffer with
//               `buffer_alloc(len)`, which only reserves CAPACITY, and never
//               set `length` — so every un-encoded chunk arrived EMPTY:
//               `chunk.length === 0`, `toString() === ""`,
//               `Buffer.concat` appended nothing.
//             * the stdin object's `on` filed the listener in a runtime-local
//               registry the event loop's has-active check cannot see (while
//               `addListener`/`once` used perry-stdlib's), so the process could
//               exit 0 with the pipe still open and the listener never fired.
//   await — `for await (const chunk of process.stdin)`, which claude-code's
//           `--input-format stream-json` reader does. `process.stdin` had no
//           `Symbol.asyncIterator` at all, so this threw instead of iterating.
import { spawn } from "node:child_process";

const ROLE_ENV = "PERRY_9399_STDIN_ROLE";
const PAYLOAD = "alpha\nbeta\n";
const role = process.env[ROLE_ENV] ?? "";

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
    console.log("chunk asyncIterator:", typeof (stream as any)[Symbol.asyncIterator]);
    console.log("chunk bytes:", total);
    console.log("chunk nonBuffer:", sawNonBuffer);
    console.log("chunk text:", JSON.stringify(Buffer.concat(chunks).toString("utf8")));
  });
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
} else if (role === "await") {
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
