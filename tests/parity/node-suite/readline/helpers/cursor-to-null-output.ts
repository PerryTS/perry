import { cursorTo } from "node:readline";

const callbacks: string[] = [];
console.log(cursorTo(null as any, 1, (error) => callbacks.push(String(error))));
await new Promise<void>((resolve) => setImmediate(resolve));
console.log(callbacks.join("|"));
