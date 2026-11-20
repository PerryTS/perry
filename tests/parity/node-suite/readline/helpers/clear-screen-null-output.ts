import { clearScreenDown } from "node:readline";

const callbacks: string[] = [];
console.log(
  clearScreenDown(undefined as any, (error) => callbacks.push(String(error))),
);
await new Promise<void>((resolve) => setImmediate(resolve));
console.log(callbacks.join("|"));
