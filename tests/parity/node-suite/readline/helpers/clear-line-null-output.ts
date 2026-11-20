import { clearLine } from "node:readline";

const callbacks: string[] = [];
console.log(
  clearLine(null as any, 0, (error) => callbacks.push(String(error))),
);
await new Promise<void>((resolve) => setImmediate(resolve));
console.log(callbacks.join("|"));
