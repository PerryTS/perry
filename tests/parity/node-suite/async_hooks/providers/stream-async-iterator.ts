import { Readable } from "node:stream";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

const result = await storage.run("stream-iterator", async () => {
  const output: string[] = [];
  const stream = Readable.from(["first", "second"]);
  for await (const chunk of stream) {
    console.log("stream iterator store:", storage.getStore(), String(chunk));
    output.push(String(chunk));
  }
  return output.join(",");
});

console.log("stream iterator result:", result);
console.log("stream iterator outside:", String(storage.getStore()));
