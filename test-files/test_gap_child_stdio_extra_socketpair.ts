// child_process `stdio[n]` (n >= 3) pipes are socketpairs: the child can read and write the
// same descriptor, and the parent can write (with callbacks and `finish`) and read it.
import { spawn } from "node:child_process";

type Pipe = any;

function readAll(stream: Pipe): Promise<string> {
  return new Promise((resolve) => {
    const parts: string[] = [];
    stream.on("data", (chunk: any) => parts.push(String(chunk)));
    stream.once("end", () => resolve(parts.join("")));
  });
}

function exited(child: any): Promise<number | null> {
  return new Promise((resolve) => child.once("close", (code: number | null) => resolve(code)));
}

// Write a spec to fd 4, end it, read the reply from fd 5.
const echo = spawn("sh", ["-c", "cat <&4 >&5"], {
  stdio: ["ignore", "pipe", "pipe", "pipe", "pipe", "pipe"],
});
const events: string[] = [];
const reply = readAll(echo.stdio[5]);
const input = echo.stdio[4] as Pipe;
input.once("finish", () => events.push("finish"));
const writeResult = input.write("hello\n", () => events.push("write-cb"));
input.end(() => events.push("end-cb"));
console.log("write returned", writeResult);
console.log("reply", JSON.stringify(await reply));
console.log("exit", await exited(echo));
console.log("events", events.join(","));
console.log("ended", input.writableEnded, input.writableFinished);

// One descriptor read and written by the child.
const duplex = spawn("sh", ["-c", "read l <&3; echo got:$l >&3"], {
  stdio: ["ignore", "pipe", "pipe", "pipe"],
});
const duplexReply = readAll(duplex.stdio[3]);
(duplex.stdio[3] as Pipe).write("ping\n");
console.log("duplex", JSON.stringify(await duplexReply));
console.log("duplex exit", await exited(duplex));

// Large writes drain through the child's reads.
const big = spawn("sh", ["-c", "cat <&4 | wc -c >&5"], {
  stdio: ["ignore", "pipe", "pipe", "pipe", "pipe", "pipe"],
});
const sizeReply = readAll(big.stdio[5]);
const chunk = "x".repeat(1024 * 1024);
(big.stdio[4] as Pipe).end(chunk);
console.log("big", (await sizeReply).trim());
console.log("big exit", await exited(big));
