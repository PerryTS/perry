import { emitKeypressEvents } from "node:readline";

const input: any = process.stdin;
const scenario = process.env.PERRY_9692_SCENARIO || "keypress";
const order = process.env.PERRY_9692_ORDER || "runtime-first";
const raw = process.env.PERRY_9692_RAW === "1";
let done = false;

input.setEncoding("utf8");

function primeRuntimeReader(stream: any, callback: (chunk: unknown) => void) {
  stream.on("data", callback);
  stream.removeListener("data", callback);
}

function primeReadlineReader(callback: (chunk: unknown) => void) {
  process.stdin.on("data", callback);
  process.stdin.removeListener("data", callback);
}

function finish(payload: string, status = 0) {
  if (done) return;
  done = true;
  if (raw && typeof input.setRawMode === "function") input.setRawMode(false);
  input.pause();
  console.log("RESULT:" + payload);
  setTimeout(() => process.exit(status), 20);
}

const timeout = setTimeout(() => finish("timeout", 2), 2500);
const prime = (_chunk: unknown) => {};
if (order === "runtime-first") primeRuntimeReader(input, prime);
else primeReadlineReader(prime);

if (raw && typeof input.setRawMode === "function") input.setRawMode(true);

if (scenario === "keypress") {
  emitKeypressEvents(input);
  const codes: number[] = [];
  process.stdin.on("keypress", (sequence: string | undefined) => {
    codes.push(sequence === undefined ? -1 : sequence.charCodeAt(0));
    if (codes.length === 3) {
      clearTimeout(timeout);
      finish("keypress:" + codes.join(","));
    }
  });
} else {
  const removed: string[] = [];
  emitKeypressEvents(input);
  process.stdin.on("keypress", () => removed.push("keypress"));
  process.stdin.on("data", () => removed.push("data"));

  if (scenario === "remove-data") input.removeAllListeners("data");
  else input.removeAllListeners();

  process.stdin.on("data", () => {
    clearTimeout(timeout);
    finish(scenario + ":" + (removed.join(",") || "none"));
  });
}

console.log("READY");
