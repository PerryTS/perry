import { type ChildProcessWithoutNullStreams, spawn } from "node:child_process";

function collect(
  child: ChildProcessWithoutNullStreams,
): Promise<{ code: number | null; stdout: string; stderr: string }> {
  return new Promise((resolve) => {
    const stdout: Buffer[] = [];
    const stderr: Buffer[] = [];
    child.stdout.on("data", (chunk: Buffer) => stdout.push(chunk));
    child.stderr.on("data", (chunk: Buffer) => stderr.push(chunk));
    child.on("close", (code: number | null) => {
      resolve({
        code,
        stdout: Buffer.concat(stdout).toString("utf8"),
        stderr: Buffer.concat(stderr).toString("utf8"),
      });
    });
  });
}

const outputSize = 128 * 1024;
const producer = spawn("node", [
  "-e",
  `process.stdout.write('o'.repeat(${outputSize})); process.stderr.write('e'.repeat(${outputSize}));`,
]);
const produced = await collect(producer);
console.log("producer status:", produced.code);
console.log("producer stdout length:", produced.stdout.length);
console.log("producer stderr length:", produced.stderr.length);
console.log(
  "producer boundaries:",
  produced.stdout[0] + produced.stdout.at(-1),
  produced.stderr[0] + produced.stderr.at(-1),
);

const consumer = spawn("node", [
  "-e",
  "let size = 0; process.stdin.on('data', chunk => size += chunk.length); process.stdin.on('end', () => process.stdout.write(String(size)));",
]);
const inputSize = 192 * 1024;
const consumed = collect(consumer);
consumer.stdin.end(Buffer.alloc(inputSize, 120));
const result = await consumed;
console.log("consumer status:", result.code);
console.log("consumer received:", result.stdout);
console.log("consumer stderr:", JSON.stringify(result.stderr));

const chunks = 6;
const chunkSize = 50 * 1024;
const multiChunk = spawn("node", [
  "-e",
  `for (let i = 0; i < ${chunks}; i++) process.stdout.write(String(i).repeat(${chunkSize}));`,
]);
const chunked = await collect(multiChunk);
console.log("multi status:", chunked.code);
console.log("multi length:", chunked.stdout.length);
console.log(
  "multi boundaries:",
  Array.from(
    { length: chunks },
    (_, index) => chunked.stdout[index * chunkSize],
  ).join(""),
);
console.log("multi stderr:", JSON.stringify(chunked.stderr));
