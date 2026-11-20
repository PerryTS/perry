import { moveCursor } from "node:readline";

const callbacks: string[] = [];
console.log(
  moveCursor(undefined as any, 1, 1, (error) => callbacks.push(String(error))),
);
await new Promise<void>((resolve) => setImmediate(resolve));
console.log(callbacks.join("|"));
